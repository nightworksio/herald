//! An event pushed to a peer's herald source (ADR-0054): as an envelope
//! proved by the instance's newest key, in a request that key signs, or
//! as it is with a CI run's ID token for the peer to prove (ADR-0064),
//! once for each revision the route shows.

use herald_core::media::JSON;
use herald_core::{
    Audited, Entry, Event, Facts, IdToken, Identity, Key, Revision, Sent, Step, Visibility,
};
use herald_kinds::{IdTokens, Presented, Proving, Pushed};
use herald_peers::answer::CONTENT_TYPE;
use herald_peers::digest::{CONTENT_DIGEST, content_digest};
use herald_peers::discovery::Instance;
use herald_peers::feed::{Envelope, identity};
use herald_peers::jcs::canonical;
use herald_peers::keys::method;
use herald_peers::message::{Covered, Message};
use herald_peers::request::demanded;
use herald_ports::{
    Algorithm, Clock, Http, Ledger, Method, Request, Secret, SecretText, Signer, Store, Versioned,
};

use super::feeding::unprovable;
use super::{Outcome, Run, Stop, stopped, stored};

/// Why an event that is not public is pushed to no peer that takes no
/// private events.
const UNSHARED: &str =
    "the event is not public, and the peer takes no private events, so it is pushed none";

/// Who answers a push.
const INBOX: &str = "the peer's inbox";

/// What a token asked of the CI is named, in place of a secret's name.
const ASKED: &str = "the run's ID token";

impl<S: Store, C: Clock, G: Signer, H: Http> Run<'_, S, C, G, H> {
    /// What the route did with `event`, pushed to the peer `pushed` names:
    /// where the routing saw it as anything but public and the peer takes
    /// no private events, withdrawn from the peer where it was pushed;
    /// left out where the route pushed it already; and otherwise pushed,
    /// seen by those the routing saw it seen by, and landed as the route's
    /// post.
    pub(crate) async fn pushed(&self, pushed: Pushed<'_>, event: &Event) -> Outcome {
        let key = Key::derive(&self.route.config.route.id, event.identity());
        let entry = match self.ports.store.ledger().entry(&key).await {
            Ok(entry) => entry,
            Err(error) => return stopped(stored(error)),
        };
        if self.seen != Visibility::Public && !pushed.private {
            return self.unshared(pushed, event, entry).await;
        }
        let instance = self.route.instance();
        let envelope = match instance.envelope(
            (event, self.seen),
            self.kinds.get(event.kind()),
            pushed.envelope,
        ) {
            Ok(envelope) => envelope,
            Err(why) => return stopped(Stop::Refused(why.to_string())),
        };
        let revision = Revision::of(
            event.signal(),
            [canonical(&envelope).unwrap_or_default().as_str()],
        );
        if let Some(unrevised) = self.unrevised(entry.as_ref(), event, &revision) {
            return unrevised;
        }
        if let Err(reason) = self
            .delivered_to(pushed, (&instance, &key), &envelope)
            .await
        {
            return stopped(reason);
        }
        let landed = self
            .fed_entry(key, entry.as_ref())
            .through(event.source())
            .revising(revision)
            .landed(
                Step::Post,
                Facts {
                    message: Some(identity(&envelope).to_owned()),
                    ..Facts::default()
                },
            );
        self.recorded(
            &landed,
            entry,
            Outcome::Sent {
                steps: vec![Step::Post],
            },
        )
        .await
    }

    /// What the route did with `event`, which the routing saw as anything
    /// but public and the peer `pushed` names takes no private events: a
    /// tombstone pushed where the route pushed it, and the route's post
    /// forgotten, so the event is pushed again once it is public; left out
    /// in any case.
    async fn unshared(
        &self,
        pushed: Pushed<'_>,
        event: &Event,
        entry: Option<Versioned<Entry>>,
    ) -> Outcome {
        let skipped = Outcome::Skipped(UNSHARED.to_owned());
        let Some(record) = entry.filter(|entry| entry.value.message(&Step::Post).is_some()) else {
            return skipped;
        };
        match self.unpushed(pushed, event.identity(), record).await {
            Outcome::Sent { .. } => skipped,
            outcome => outcome,
        }
    }

    /// What the route did with `identity`, which it pushed and whose post
    /// `record` keeps: a tombstone of it pushed to the peer `pushed` names,
    /// and its post forgotten, so a re-created release is pushed again.
    pub(super) async fn unpushed(
        &self,
        pushed: Pushed<'_>,
        identity: &Identity,
        record: Versioned<Entry>,
    ) -> Outcome {
        let instance = self.route.instance();
        let tombstone = instance.tombstone(identity.as_str(), self.ports.clock.now());
        let key = record.value.key;
        if let Err(reason) = self
            .delivered_to(pushed, (&instance, &key), &tombstone)
            .await
        {
            return stopped(reason);
        }
        let forgotten = self.fed_entry(key, None);
        let withdrawn = Outcome::Sent {
            steps: vec![Step::Delete(0)],
        };
        self.recorded(&forgotten, Some(record), withdrawn).await
    }

    /// `envelope` of the announcement `key` keys, pushed to the peer
    /// `pushed` names: proved by the instance's newest key in a request
    /// that key signs, under a nonce no other request takes; or as it is,
    /// with the run's ID token, for the peer to prove (ADR-0064). Done once
    /// the peer's inbox took it.
    async fn delivered_to(
        &self,
        pushed: Pushed<'_>,
        (instance, key): (&Instance<'_>, &Key),
        envelope: &Envelope,
    ) -> Result<(), Stop> {
        let mut spent = None;
        let (body, request) = match pushed.proving {
            Proving::Key(secret) => {
                self.signed((pushed.inbox, secret), (instance, key), envelope)
                    .await?
            }
            Proving::IdToken { tokens, audience } => {
                let (token, read) = self.id_token(tokens, audience).await?;
                if !IdToken::read(token.expose()).is_ok_and(|read| read.meant_for(audience)) {
                    return Err(Stop::Refused(format!(
                        "the run's ID token is not meant for {audience}, so it is not sent"
                    )));
                }
                let body = envelope.to_string().into_bytes();
                let request = Request::new(Method::Post, SecretText::plain(pushed.inbox))
                    .header(CONTENT_TYPE, SecretText::plain(JSON))
                    .bearer(token.clone());
                spent = read.map(|read| (read, token));
                (body, request)
            }
        };
        let response = self
            .http
            .send(Request {
                body,
                most: Audited::NAMED_MOST,
                ..request
            })
            .await
            .map_err(|error| {
                if let Some((read, token)) = spent.filter(|_| error.unsent()) {
                    read.unspent(&token);
                }
                Stop::Unreachable(error.to_string())
            })?;
        let said = || Audited::named(&String::from_utf8_lossy(&response.body));
        match response.failure(pushed.waits, None, INBOX, said) {
            None | Some(Sent::Landed(_)) => Ok(()),
            Some(Sent::RetryAfter(after)) => Err(Stop::RetryAfter(after)),
            Some(Sent::Refused(reason)) => Err(Stop::Refused(reason)),
        }
    }

    /// The run's ID token for one push, meant for `audience`: asked of the
    /// CI afresh, or the first of those read from its variables no push
    /// presented, with what holds them presented.
    ///
    /// # Errors
    ///
    /// The CI does not answer one, or every one read was presented.
    async fn id_token<'t>(
        &self,
        tokens: IdTokens<'t>,
        audience: &str,
    ) -> Result<(Secret, Option<&'t Presented>), Stop> {
        let (url, token) = match tokens {
            IdTokens::Requested { url, token } => (url, token),
            IdTokens::Read { tokens, presented } => {
                let token = presented.next(tokens).ok_or_else(|| {
                    Stop::Limit(
                        "every ID token the run read was presented once; a later run pushes the rest, or give the run one more in id_tokens for each push".to_owned(),
                    )
                })?;
                return Ok((token, Some(presented)));
            }
        };
        let asked = SecretText::secret(url.clone()).then(IdToken::asking(url.expose(), audience));
        let response = self
            .http
            .send(
                Request::new(Method::Get, asked)
                    .header("Accept", SecretText::plain(JSON))
                    .bearer(token.clone())
                    .at_most(IdToken::MOST.saturating_mul(2)),
            )
            .await
            .map_err(|error| Stop::Unreachable(format!("the run's ID token: {error}")))?;
        if response.status != herald_core::status::OK {
            return Err(Stop::Refused(format!(
                "the CI answered {} when asked for the run's ID token",
                response.status
            )));
        }
        IdToken::answered(&response.body)
            .map(|value| (Secret::new(ASKED, value), None))
            .ok_or_else(|| {
                Stop::Refused("the CI answered no ID token when asked for one".to_owned())
            })
    }

    /// `envelope` of the announcement `key` keys, proved by the instance's
    /// newest key, `secret`, and the request to `inbox` carrying it, which
    /// that key signs under a nonce no other request takes.
    async fn signed(
        &self,
        (inbox, secret): (&str, &Secret),
        (instance, key): (&Instance<'_>, &Key),
        envelope: &Envelope,
    ) -> Result<(Vec<u8>, Request), Stop> {
        let public = self.public(secret).await?;
        let proved = self.proved((secret, &public), instance, envelope).await?;
        let body = proved.to_string().into_bytes();
        let now = self.ports.clock.now();
        let created = now.as_second();
        let mut fields = vec![
            (CONTENT_TYPE.to_owned(), JSON.to_owned()),
            (CONTENT_DIGEST.to_owned(), content_digest(&body)),
        ];
        let covered = Covered {
            components: demanded().to_vec(),
            created,
            expires: created.saturating_add(self.route.signed_for.as_secs()),
            nonce: Some(
                key.revised(&format!("{}.{}", self.holder, now.as_nanosecond()))
                    .to_string(),
            ),
            keyid: method(instance.domain, &public),
        };
        let message = Message {
            method: "POST",
            url: inbox,
            status: None,
            fields: &fields,
        };
        let base = covered.base(&message).map_err(unprovable)?;
        let signature = self
            .ports
            .signer
            .sign(Algorithm::Ed25519, secret, &base)
            .await
            .map_err(unprovable)?;
        fields.extend(covered.fields(&signature));
        let request = fields.into_iter().fold(
            Request::new(Method::Post, SecretText::plain(inbox)),
            |request, (name, value)| request.header(name, SecretText::plain(&value)),
        );
        Ok((body, request))
    }
}
