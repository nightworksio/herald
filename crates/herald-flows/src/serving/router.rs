//! Where each request goes: the paths herald serves under its base, and the
//! part of the server that answers each.

use herald_core::Escaping;
use herald_core::status;
use herald_kinds::Endpoint;
use herald_ports::{Clock, Http, Keyring, Signer, Store, Verifier};

use super::actors::{actor, counted, documented, domain, fingered, inboxed, noted, outboxed};
use super::apps::{posted, standing};
use super::replies::{Answered, Reply, missing, text_reply, unmethodical};
use super::webhooks::hooked;
use super::{Inbound, Serving};
use crate::command_endpoints::{INTERACTIONS, MATRIX, commanded};

/// Where the server answers whether it is up.
pub const HEALTH: &str = "/healthz";
/// What the server answers at [`HEALTH`].
const UP: &str = "herald is up";
/// Where a forge delivers its webhooks, before the source's name.
pub(crate) const HOOKS: &str = "/hooks/";
/// Where apps post their projects' events: `/api/projects/<project>/events`.
pub const PROJECTS: &str = "/api/projects/";
/// Where WebFinger is asked.
const WEBFINGER: &str = "/.well-known/webfinger";
/// The inbox every actor herald serves shares.
const SHARED_INBOX: &str = "/inbox";
/// Where herald's actors are served, before an actor's name.
const USERS: &str = "/users/";

/// The reply to `method` on `target` that needs no composed herald, where
/// there is one: whether it is up, at [`HEALTH`] under `base`. A host that
/// composes herald per request answers it once its config loads, before it
/// opens a store.
#[must_use]
pub fn uncomposed(method: &str, target: &str, base: &str) -> Option<Reply> {
    let path = target.split_once('?').map_or(target, |(path, _)| path);
    (method == "GET" && under(base, path) == Some(HEALTH)).then(|| text_reply(status::OK, UP).reply)
}

/// `path` as herald routes it, where it lies under `base`, the path every
/// endpoint is served under: `base` taken off, and the root where nothing
/// is left.
#[must_use]
pub fn under<'a>(base: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(base)?;
    match rest {
        "" => Some("/"),
        rest if rest.starts_with('/') => Some(rest),
        _ => None,
    }
}

/// The reply to `inbound`, and the work it leaves: health, forge webhooks,
/// and herald's ActivityPub actors, their Notes, inboxes and WebFinger
/// (ADR-0002). Every webhook and activity is verified and admitted before
/// it is parsed (ADR-0007, ADR-0013), and its work is left for after the
/// reply.
pub async fn answer<S: Store, C: Clock, G: Signer, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(path) = under(&serving.config.server.base, inbound.path) else {
        return missing("such path");
    };
    match (inbound.method, path) {
        ("GET", HEALTH) => text_reply(status::OK, UP),
        ("GET", WEBFINGER) => fingered(serving.connected, inbound.query),
        ("POST", SHARED_INBOX) => match domain(serving.connected, inbound.host()) {
            Some(audience) => inboxed(serving, inbound, audience).await,
            None => missing("actor on this host"),
        },
        (_, HEALTH | WEBFINGER | SHARED_INBOX) => unmethodical(),
        (method, path) => prefixed(serving, method, path, inbound).await,
    }
}

/// The reply to `method` on `path`, a path under one of the prefixes the
/// server routes by: the HTTP API, command endpoints, webhooks and actors.
async fn prefixed<S: Store, C: Clock, G: Signer, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    method: &str,
    path: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    if let Some(rest) = path.strip_prefix(PROJECTS) {
        return projects(serving, method, rest, inbound).await;
    }
    if let Some(destination) = path.strip_prefix(INTERACTIONS) {
        return if method == "POST" {
            commanded(serving, Endpoint::Posted, destination, "", inbound).await
        } else {
            unmethodical()
        };
    }
    if let Some((destination, rest)) = path
        .strip_prefix(MATRIX)
        .and_then(|rest| rest.split_once('/'))
    {
        return commanded(serving, Endpoint::Pushed, destination, rest, inbound).await;
    }
    if let Some(source) = path.strip_prefix(HOOKS) {
        return if method == "POST" {
            hooked(serving, source, inbound).await
        } else {
            unmethodical()
        };
    }
    match path.strip_prefix(USERS) {
        Some(rest) => users(serving, method, rest, inbound).await,
        None => missing("such path"),
    }
}

/// The reply to `method` on `rest`, the path after [`PROJECTS`]: an app's
/// event posted, or where one stands.
async fn projects<S: Store, C: Clock, G, V, H, P, K>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    method: &str,
    rest: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let Some(segments) = rest
        .split('/')
        .map(|segment| Escaping::UNRESERVED.unescape(segment))
        .collect::<Option<Vec<_>>>()
    else {
        return missing("such path");
    };
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
    match (method, segments.as_slice()) {
        ("POST", [project, "events"]) => posted(serving, project, inbound).await,
        ("GET", [project, "events", id]) => standing(serving, project, id, inbound).await,
        (_, [_, "events"] | [_, "events", _]) => unmethodical(),
        _ => missing("such path"),
    }
}

/// The reply to `method` on `rest`, the path after [`USERS`]: an actor's
/// document, inbox, Notes, outbox or followers.
async fn users<S: Store, C: Clock, G: Signer, V: Verifier, H, P: Http, K: Keyring>(
    serving: &Serving<'_, S, C, G, V, H, P, K>,
    method: &str,
    rest: &str,
    inbound: &Inbound<'_>,
) -> Answered {
    let parts: Vec<&str> = rest.split('/').collect();
    let Some((served, destination)) = parts
        .first()
        .and_then(|name| actor(serving.connected, name, inbound.host()))
    else {
        return missing("such actor");
    };
    match (method, parts.as_slice()) {
        ("GET", [_]) => documented(serving, served).await,
        ("POST", [_, "inbox"]) => inboxed(serving, inbound, served.domain()).await,
        ("GET", [_, "notes", key]) => noted(serving, served, key).await,
        ("GET", [_, "outbox"]) => outboxed(serving, served, inbound.query).await,
        ("GET", [_, "followers"]) => counted(serving, served, destination).await,
        (_, [_] | [_, "inbox" | "outbox" | "followers"] | [_, "notes", _]) => unmethodical(),
        _ => missing("such path"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whether_it_is_up_is_answered_uncomposed() {
        let up = uncomposed("GET", "/healthz", "").unwrap();
        assert_eq!((up.status, up.body.as_slice()), (200, UP.as_bytes()));
        assert_eq!(uncomposed("GET", "/healthz?probe=1", ""), Some(up.clone()));
        assert_eq!(uncomposed("POST", "/healthz", ""), None);
        assert_eq!(uncomposed("GET", "/healthz/more", ""), None);
        assert_eq!(uncomposed("GET", "/inbox", ""), None);
        assert_eq!(uncomposed("GET", "/herald/healthz", "/herald"), Some(up));
        assert_eq!(uncomposed("GET", "/healthz", "/herald"), None);
        assert_eq!(uncomposed("GET", "/heraldhealthz", "/herald"), None);
    }

    #[test]
    fn a_path_is_routed_where_it_lies_under_the_base() {
        assert_eq!(under("", "/hooks/gate"), Some("/hooks/gate"));
        assert_eq!(under("/herald", "/herald/hooks/gate"), Some("/hooks/gate"));
        assert_eq!(under("/herald", "/herald"), Some("/"));
        assert_eq!(under("/herald", "/heraldry/hooks"), None);
        assert_eq!(under("/herald", "/hooks/gate"), None);
    }
}
