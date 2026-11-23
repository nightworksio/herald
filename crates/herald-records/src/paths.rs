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

pub(crate) const CODEOWNERS: &str = "codeowners";

/// The most bytes a kept CODEOWNERS file's segment is written with before
/// its digest names it instead (ADR-0076).
const MOST_NAMED: usize = 128;

/// Where the CODEOWNERS file keyed `key` is kept: every file in one
/// directory, its source, repository and branch written as one segment,
/// or past [`MOST_NAMED`] bytes as that segment's digest, which holds no
/// `~` where the segment holds two.
pub(crate) fn codeowners(key: &herald_core::CodeownersKey) -> String {
    let reference = key
        .reference
        .as_ref()
        .map_or_else(String::new, |reference| format!("@{reference}"));
    let named = segment(&format!(
        "{}\n{}\n{reference}",
        key.source.as_str(),
        key.repository
    ));
    if named.len() > MOST_NAMED {
        return format!("{CODEOWNERS}/{}.json", herald_core::digest(&named));
    }
    format!("{CODEOWNERS}/{named}.json")
}

pub(crate) const BASES: &str = "bases";

/// Where the base branch keyed `key` is kept: every branch in one
/// directory, its source, repository and branch written as one segment,
/// or past [`MOST_NAMED`] bytes as that segment's digest (ADR-0083).
pub(crate) fn base(key: &herald_core::BaseKey) -> String {
    let named = segment(&format!(
        "{}\n{}\n{}",
        key.source.as_str(),
        key.repository,
        key.branch
    ));
    if named.len() > MOST_NAMED {
        return format!("{BASES}/{}.json", herald_core::digest(&named));
    }
    format!("{BASES}/{named}.json")
}

/// Where the reconcile of `source` stands.
pub(crate) fn reconciled(source: &SourceName) -> String {
    format!("reconciled/{}.json", segment(source.as_str()))
}

/// Where the names of the reports whose living posts were sent advisories
/// are kept (ADR-0067).
pub(crate) const RECALLS: &str = "recalls.json";

/// Where the record of the re-run of the run `key` is kept: every re-run
/// in one directory, its key written as one segment (ADR-0068).
pub(crate) fn action(key: &herald_core::RunKey) -> String {
    format!("actions/{}.json", segment(&key.to_string()))
}

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

    #[test]
    fn a_codeowners_file_is_kept_by_source_repository_and_branch_each_apart() {
        let key = |source: &str, repository: &str, reference: Option<&str>| {
            codeowners(&herald_core::CodeownersKey {
                source: SourceName::new(source),
                repository: repository.to_owned(),
                reference: reference.map(str::to_owned),
            })
        };
        assert_eq!(
            key("s", "o/r", Some("main")),
            "codeowners/s~0ao~2fr~0a~40main.json"
        );
        assert_eq!(key("s", "o/r", None), "codeowners/s~0ao~2fr~0a.json");
        assert_ne!(key("s", "o/r", Some("")), key("s", "o/r", None));
        assert_ne!(key("s/o", "r", None), key("s", "o/r", None));
    }

    #[test]
    fn a_codeowners_file_named_past_the_most_bytes_is_kept_by_its_digest() {
        let key = |reference: &str| {
            codeowners(&herald_core::CodeownersKey {
                source: SourceName::new("s"),
                repository: "o/r".to_owned(),
                reference: Some(reference.to_owned()),
            })
        };
        let start = segment("s\no/r\n@");
        let most = "a".repeat(MOST_NAMED - start.len());
        assert_eq!(key(&most), format!("codeowners/{start}{most}.json"));
        let past = format!("{most}a");
        let named = format!("{start}{past}");
        assert_eq!(
            key(&past),
            format!("codeowners/{}.json", herald_core::digest(&named))
        );
        let long = "ü".repeat(200);
        assert_eq!(
            key(&long),
            format!(
                "codeowners/{}.json",
                herald_core::digest(&segment(&format!("s\no/r\n@{long}")))
            )
        );
        assert_ne!(key(&long), key(&"ö".repeat(200)));
    }
}
