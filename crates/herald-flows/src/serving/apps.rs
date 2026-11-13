//! The HTTP API for apps: an app's event posted to its project, and where
//! its announcement stands, each request's token and project checked before
//! it is read (ADR-0007, ADR-0015).

use herald_config::{App, Config};
use herald_core::status;
use herald_core::{Identity, Key, Standing, digest, same};
use herald_ports::{Clock, Ledger, Store};

use super::jobs::Job;
use super::replies::{Answered, refusal, rejection, replied, text_reply};
use super::{Inbound, Serving};
use crate::verification::{Rejected, Verified, admitted};

/// The media type of the HTTP API's answers.
const JSON: &str = "application/json";

/// An app's event posted to `project`: its token and the project are checked
/// before the event is read, the event is admitted once, and its
/// announcement is left to do (ADR-0007, ADR-0013).
pub(super) async fn posted<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    project: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let (name, _) = match scoped(serving.config, project, inbound) {
        Ok(scoped) => scoped,
        Err(refused) => return refused.answered(),
    };
    let now = serving.ports.clock.now();
    let event = match herald_sources_app::event(name, inbound.body, now, &serving.config.kinds) {
        Ok(event) => event,
        Err(unread) => return refusal(status::BAD_REQUEST, unread.to_string()),
    };
    if event.project() != project {
        return refusal(
            status::BAD_REQUEST,
            format!(
                "the event names the project \"{}\", not \"{project}\"",
                event.project()
            ),
        );
    }
    let verified = Verified {
        delivery: Some(event.identity().as_str().to_owned()),
        signer: Some(name.to_owned()),
    };
    match admitted(
        serving.ports.store,
        serving.admission,
        herald_sources_app::SCHEME,
        &verified,
        now,
    )
    .await
    {
        Ok(()) => Answered {
            reply: text_reply(status::ACCEPTED, "herald announces the event").reply,
            refused: None,
            job: Some(Job::App {
                app: name.to_owned(),
                event,
                verified,
            }),
        },
        Err(Rejected::Replayed) => text_reply(status::OK, "herald has this event already"),
        Err(rejected) => rejection(&rejected),
    }
}

/// Why an app's request is refused before it is read.
enum Refused {
    /// It presents no token herald knows.
    Token,
    /// Its token may not announce the project it names.
    Project,
}

impl Refused {
    fn answered(self) -> Answered {
        match self {
            Self::Token => refusal(
                status::UNAUTHORIZED,
                "the request presents no token herald knows",
            ),
            Self::Project => refusal(status::FORBIDDEN, "the token may not announce this project"),
        }
    }
}

/// The app whose token `inbound` presents, where it may announce
/// `project`, or why it is refused.
fn scoped<'c>(
    config: &'c Config,
    project: &str,
    inbound: &Inbound<'_>,
) -> Result<(&'c str, &'c App), Refused> {
    let token = inbound
        .header("authorization")
        .and_then(herald_core::bearer);
    let Some((name, app)) = token.and_then(|token| token_owner(config, token)) else {
        return Err(Refused::Token);
    };
    if !app.projects.iter().any(|named| named == project) {
        return Err(Refused::Project);
    }
    Ok((name, app))
}

/// Where the announcement of the event its app gave `id` stands on each
/// route the app may announce to, read from the ledger (ADR-0015).
pub(super) async fn standing<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    project: &str,
    id: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let (name, app) = match scoped(serving.config, project, inbound) {
        Ok(scoped) => scoped,
        Err(refused) => return refused.answered(),
    };
    let identity = Identity::new(herald_sources_app::identity(name, id));
    let now = serving.ports.clock.now();
    let mut routes = Vec::new();
    for route in &app.routes {
        let key = Key::derive(route, &identity);
        let standing = match serving.ports.store.ledger().entry(&key).await {
            Ok(Some(entry)) => entry.value.standing(now),
            Ok(None) => Standing::Pending,
            Err(error) => return refusal(status::UNAVAILABLE, error.to_string()),
        };
        routes.push((route.as_str(), standing));
    }
    replied(
        status::OK,
        JSON,
        herald_sources_app::standings(id, project, &routes),
    )
}

/// The app one of whose tokens is `token`, its SHA-256 compared with every
/// hash every app keeps, each in constant time (ADR-0007).
fn token_owner<'c>(config: &'c Config, token: &str) -> Option<(&'c str, &'c App)> {
    let presented = digest(token);
    let mut found = None;
    for (name, app) in &config.apps {
        for held in &app.tokens {
            if same(held.as_bytes(), presented.as_bytes()) {
                found = Some((name.as_str(), app));
            }
        }
    }
    found
}
