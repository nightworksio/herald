use herald_core::Sent;
use herald_kinds::{Activity, Federates, Inboxes};
use herald_ports::{Boxed, CryptoError, DynHttp, DynSigner, Erased, HttpError};
use jiff::Timestamp;
use serde_json::Value;

use super::deliver::ActivityPub;
use super::documents::{FIRST, InboxActivity, asked, follow_id};
use crate::actor::inboxes;

impl Federates for ActivityPub {
    fn actor(&self) -> &str {
        Self::actor(self)
    }

    fn domain(&self) -> &str {
        Self::domain(self)
    }

    fn id(&self) -> String {
        Self::id(self)
    }

    fn finger(&self) -> String {
        Self::finger(self)
    }

    fn document<'a>(&'a self, signer: &'a dyn DynSigner) -> Boxed<'a, Result<String, CryptoError>> {
        Box::pin(async move { Self::document(self, &Erased(signer)).await })
    }

    fn owns(&self, note: &str, key: &str) -> bool {
        Self::owns(self, note, key)
    }

    fn outbox_most(&self) -> usize {
        Self::outbox_most(self)
    }

    fn deliveries(&self) -> usize {
        Self::deliveries(self)
    }

    fn first_page(&self) -> &'static str {
        FIRST
    }

    fn outbox(&self, total: usize) -> String {
        Self::outbox(self, total)
    }

    fn outbox_page(&self, page: &str, notes: &[String], next: Option<&str>) -> String {
        Self::outbox_page(self, page, notes, next)
    }

    fn followers(&self, admitted: usize) -> String {
        Self::followers(self, admitted)
    }

    fn admits(&self, follower: &str) -> bool {
        self.policy().admits(follower)
    }

    fn room(&self, follower: &str, following: &[&str]) -> bool {
        self.policy().room(follower, following)
    }

    fn manual(&self) -> bool {
        self.policy().manual()
    }

    fn read(&self, body: &[u8], signer: &str) -> Activity {
        match asked(body, signer) {
            InboxActivity::Follow { target, follow } => Activity::Follow {
                target,
                id: follow_id(&follow),
            },
            InboxActivity::Undo { follower, target } => Activity::Undo { follower, target },
            InboxActivity::Nothing(why) => Activity::Nothing(why),
        }
    }

    fn inboxes(&self, document: &[u8], follower: &str) -> Option<Inboxes> {
        let found = inboxes(document, follower)?;
        Some(Inboxes {
            delivered: found.delivered().to_owned(),
            personal: found.personal,
        })
    }

    fn answer(&self, id: &str, follower: &str, accept: bool) -> String {
        let follow = self.follow_of(id, follower);
        if accept {
            self.accept(&follow)
        } else {
            self.reject(&follow)
        }
        .to_string()
    }

    fn post<'a>(
        &'a self,
        http: &'a dyn DynHttp,
        signer: &'a dyn DynSigner,
        now: Timestamp,
        inbox: &'a str,
        answer: &'a str,
    ) -> Boxed<'a, Result<Sent, HttpError>> {
        Box::pin(async move {
            let activity: Value = serde_json::from_str(answer).unwrap_or(Value::Null);
            Self::post(self, &Erased(http), &Erased(signer), now, inbox, &activity).await
        })
    }
}
