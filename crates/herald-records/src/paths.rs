use herald_core::{
    DestinationName, Escaping, Hold, Key, RouteId, Schedule, SourceName, Subscription,
};
use herald_ports::Outboxed;
use jiff::Timestamp;

/// A name as one segment of a path: letters, digits, `.`, `_` and `-` kept,
/// and every other byte written as `~` and its two hex digits.
const NAME: Escaping = Escaping {
    keeps: |byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte),
    mark: '~',
    upper: false,
};

pub(crate) fn segment(name: &str) -> String {
    NAME.escape(name)
}

pub(crate) const LEDGER: &str = "ledger";

pub(crate) fn entry(key: &Key) -> String {
    format!("{LEDGER}/{key}.json")
}

/// The directory of every delivery of `key` to a follower's inbox.
pub(crate) fn inboxes(key: &Key) -> String {
    format!("inboxes/{key}")
}

/// Where the delivery of `key` to `inbox` is kept: under the inbox's
/// digest, so a long inbox URL makes no long path.
pub(crate) fn inbox(key: &Key, inbox: &str) -> String {
    format!("{}/{}.json", inboxes(key), herald_core::digest(inbox))
}

pub(crate) fn cursor(source: &SourceName) -> String {
    format!("cursors/{}.json", segment(source.as_str()))
}

/// The directory of every member of the owner source `source`.
pub(crate) fn members(source: &SourceName) -> String {
    format!("cursors/{}", segment(source.as_str()))
}

/// Where the member of `source` whose forge id is `id` is kept, with its
/// cursor.
pub(crate) fn member(source: &SourceName, id: &str) -> String {
    format!("{}/{}.json", members(source), segment(id))
}

pub(crate) const PENDING: &str = "pending";

/// Where the events `hold` holds back are kept: under its digest.
pub(crate) fn pending(hold: &Hold) -> String {
    format!("{PENDING}/{}.json", hold.digest())
}

pub(crate) fn token(name: &str) -> String {
    format!("tokens/{}.json", segment(name))
}

/// Where the key set the instance last published is kept.
pub(crate) const PUBLISHED_KEYS: &str = "published/keys.json";

/// Where the static signed log the instance last wrote is kept.
pub(crate) const PUBLICATION: &str = "published/log.json";

/// Where the envelopes the feed of `destination` lists are kept.
pub(crate) fn feed(destination: &DestinationName) -> String {
    format!("published/feeds/{}.json", segment(destination.as_str()))
}

/// Where the key set the peer `peer` last handed over to is kept.
pub(crate) fn peer_keys(peer: &str) -> String {
    format!("peers/{}.json", segment(peer))
}

/// The directory of what every peer asked, by day.
pub(crate) const AUDITS: &str = "audits";

/// Where what `peer` asked on `day` is kept: in one directory with every
/// other peer's days, which a peer's name, holding no `.`, keeps apart.
pub(crate) fn audit(peer: &str, day: &str) -> String {
    format!("{AUDITS}/{}.{day}.json", segment(peer))
}

/// The day the audit kept at `path` is of, where it is one.
pub(crate) fn audit_day(path: &str) -> Option<&str> {
    path.strip_prefix(AUDITS)?
        .strip_prefix('/')?
        .strip_suffix(".json")?
        .rsplit_once('.')
        .map(|(_, day)| day)
}

/// Where the window of `peer`'s rate is kept.
pub(crate) fn window(peer: &str) -> String {
    format!("windows/{}.json", segment(peer))
}

pub(crate) fn listing(source: &SourceName) -> String {
    format!("listings/{}.json", segment(source.as_str()))
}

pub(crate) const SCHEDULES: &str = "schedules";

pub(crate) fn schedule(schedule: &Schedule) -> String {
    format!(
        "{SCHEDULES}/{}.{}.{:?}.json",
        schedule.key,
        segment(&schedule.thread),
        schedule.chore
    )
}

pub(crate) fn subscribers(destination: &DestinationName, project: &str) -> String {
    format!(
        "subscriptions/{}/{}",
        segment(destination.as_str()),
        segment(project)
    )
}

pub(crate) fn subscribed(subscription: &Subscription) -> String {
    format!(
        "{}/{}.json",
        subscribers(&subscription.destination, &subscription.project),
        segment(&subscription.subscriber)
    )
}

pub(crate) const DELIVERIES: &str = "deliveries";

pub(crate) fn delivery(namespace: &str, id: &str) -> String {
    format!("{DELIVERIES}/{namespace}.{}.json", segment(id))
}

pub(crate) const NOTES: &str = "notes";

pub(crate) fn note(key: &Key) -> String {
    format!("{NOTES}/{key}.json")
}

/// The directory of `actor`'s outbox: one record per Note, named so that
/// names sort as the Notes were published.
pub(crate) fn outbox(actor: &str) -> String {
    format!("outbox/{}", segment(actor))
}

/// Where `actor`'s outbox lists the Note of `key`, published at `at`.
pub(crate) fn outboxed(actor: &str, at: Timestamp, key: &Key) -> String {
    format!("{}/{}.json", outbox(actor), Outboxed::name(at, key))
}

/// The name an outbox lists at `path`, without its directory or `.json`.
pub(crate) fn outboxed_name(path: &str) -> Option<String> {
    let name = path.rsplit('/').next()?.strip_suffix(".json")?;
    Some(name.to_owned())
}

pub(crate) const LATEST: &str = "latest";

pub(crate) fn latest(project: &str) -> String {
    format!("{LATEST}/{}.json", segment(project))
}

pub(crate) const KEYS: &str = "keys";

pub(crate) fn key(id: &str) -> String {
    format!("{KEYS}/{}.json", segment(id))
}

pub(crate) fn pin(route: &RouteId) -> String {
    format!("pins/{}.json", segment(route.as_str()))
}

pub(crate) const DECISIONS: &str = "decisions";

pub(crate) fn decision(key: &herald_core::Key) -> String {
    format!("{DECISIONS}/{key}.json")
}

pub(crate) const PULLS: &str = "pulls";

/// Where the pull request keyed `key` is kept: every pull request in one
/// directory, its key written as one segment, so a repository's pull
/// requests share the start [`repository_pulls`] gives.
pub(crate) fn pull(key: &herald_core::PullKey) -> String {
    format!("{PULLS}/{}.json", segment(&key.to_string()))
}

/// The start of the path of every pull request of `repository` kept.
pub(crate) fn repository_pulls(repository: &herald_core::RepositoryKey) -> String {
    format!("{PULLS}/{}", segment(&format!("{repository}/")))
}

/// Where the reconcile of `source` stands.
pub(crate) fn reconciled(source: &SourceName) -> String {
    format!("reconciled/{}.json", segment(source.as_str()))
}

/// Where the names of the reports whose living posts were sent advisories
/// are kept (ADR-0067).
pub(crate) const RECALLS: &str = "recalls.json";

pub(crate) fn position(destination: &DestinationName) -> String {
    format!("positions/{}.json", segment(destination.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_becomes_one_safe_segment() {
        assert_eq!(segment("php-mutation-gate_1.0"), "php-mutation-gate_1.0");
        assert_eq!(segment("a/b c~"), "a~2fb~20c~7e");
        assert_eq!(segment("é"), "~c3~a9");
    }
}
