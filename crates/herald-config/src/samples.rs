use std::collections::BTreeMap;

use herald_core::{Draft, Event, Kind};
use jiff::Timestamp;

/// Noon on 31 October 2026, when every sample happened.
const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);
/// The lines of a sample's long body, past every destination's limit.
const LONG: usize = 2_000;

fn draft(kind: Kind) -> Draft {
    let version = match kind {
        Kind::Release | Kind::PackageVersion => Some("v1.2.0".to_owned()),
        Kind::FeedEntry | Kind::Message => None,
    };
    Draft {
        identity: format!("sample/{kind:?}"),
        kind,
        project: "sample-project".to_owned(),
        version,
        title: "Sample project 1.2.0".to_owned(),
        body: "## Added\n\n- a sample change".to_owned(),
        links: BTreeMap::from([
            (
                "release".to_owned(),
                "https://example.org/releases/v1.2.0".to_owned(),
            ),
            (
                "changelog".to_owned(),
                "https://example.org/CHANGELOG.md".to_owned(),
            ),
        ]),
        prerelease: false,
        occurred: NOON,
    }
}

/// The sample events of `kind` every template is dry-rendered against: a
/// plain one, a pre-release and one with a long body (ADR-0006).
#[must_use]
pub fn samples(kind: Kind) -> Vec<Event> {
    let plain = draft(kind);
    let prerelease = Draft {
        version: plain
            .version
            .as_ref()
            .map(|version| format!("{version}-rc.1")),
        prerelease: true,
        ..plain.clone()
    };
    let long = Draft {
        body: "- a sample change\n".repeat(LONG),
        ..plain.clone()
    };
    [plain, prerelease, long]
        .into_iter()
        .filter_map(|draft| Event::normalise(draft).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_plain_a_pre_release_and_a_long_sample() {
        for kind in [
            Kind::Release,
            Kind::PackageVersion,
            Kind::FeedEntry,
            Kind::Message,
        ] {
            let samples = samples(kind);
            assert_eq!(samples.len(), 3, "{kind:?}");
            assert!(samples.iter().all(|event| event.kind() == kind));
            assert!(samples[1].prerelease());
            assert!(samples[2].body().len() > 30_000);
        }
        assert_eq!(samples(Kind::FeedEntry)[0].version(), None);
        assert_eq!(samples(Kind::Release)[1].version(), Some("v1.2.0-rc.1"));
    }
}
