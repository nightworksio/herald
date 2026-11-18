use std::collections::BTreeMap;

use herald_core::{
    Attribute, AttributeType, Board, Checks, Draft, Event, Kind, KindSpec, Mergeable, Origin,
    PRERELEASE, Pull, Review, SIGNAL, Signal, SourceName, VERSION, View, Visibility, board,
};
use jiff::Timestamp;

/// Noon on 31 October 2026, when every sample happened.
const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);
/// The lines of a sample's long body, past every destination's limit.
const LONG: usize = 2_000;

/// The signal a sample of the kind `kind` is of: a package version's, or
/// a Release's.
fn signal_of(kind: &Kind) -> Signal {
    if *kind == Kind::PACKAGE_VERSION {
        Signal::Package
    } else {
        Signal::Release
    }
}

fn draft(spec: &KindSpec) -> Draft {
    let attributes = spec
        .attributes
        .iter()
        .map(|(name, attribute)| {
            let sample = match attribute.ty {
                AttributeType::Text if name == VERSION => Attribute::Text("v1.2.0".to_owned()),
                AttributeType::Text if name == SIGNAL => {
                    Attribute::Text(signal_of(&spec.kind).name().to_owned())
                }
                AttributeType::Text => Attribute::Text(format!("sample {name}")),
                AttributeType::Flag => Attribute::Flag(false),
                AttributeType::Number => Attribute::Number(1.into()),
                AttributeType::Url => Attribute::Text(format!("https://example.org/{name}")),
            };
            (name.clone(), sample)
        })
        .collect();
    Draft {
        identity: format!("sample/{}", spec.kind),
        kind: spec.kind.clone(),
        project: "sample-project".to_owned(),
        title: "Sample project 1.2.0".to_owned(),
        body: "## Added\n\n- a sample change".to_owned(),
        links: spec
            .links
            .iter()
            .map(|link| {
                (
                    link.name.clone(),
                    format!("https://example.org/{}", link.name),
                )
            })
            .collect(),
        attributes,
        occurred: NOON,
    }
}

/// The boards every report template is dry-rendered against: one with a
/// person's pull request waiting on review and a bot's bump, and an empty
/// one.
#[must_use]
pub fn sample_boards() -> [Board; 2] {
    let opened = NOON - jiff::SignedDuration::from_hours(15 * 24);
    let pull = Pull {
        project: "sample-project".to_owned(),
        repository: "sample-owner/sample-project".to_owned(),
        visibility: Visibility::Public,
        number: 1,
        title: "Add a sample".to_owned(),
        url: "https://github.com/sample-owner/sample-project/pull/1".to_owned(),
        author: Some("sample-author".to_owned()),
        bot: false,
        draft: false,
        labels: vec!["sample-label".to_owned()],
        opened,
        updated: opened,
        review: Review::Requested,
        reviewers: vec!["sample-reviewer".to_owned()],
        checks: Checks::Passed,
        mergeable: Mergeable::Clean,
        queued: false,
        forge: None,
        head: Some("0123456789abcdef".to_owned()),
        unread: false,
    };
    let bump = Pull {
        number: 2,
        title: "Bump sample-package from 1.0.0 to 1.1.0".to_owned(),
        url: "https://github.com/sample-owner/sample-project/pull/2".to_owned(),
        author: Some("dependabot[bot]".to_owned()),
        bot: true,
        review: Review::Approved,
        ..pull.clone()
    };
    [
        board(&View::default(), &[pull, bump], NOON),
        Board::default(),
    ]
}

/// Where a sample event from a forge, registry or app that names one comes
/// from.
#[must_use]
pub fn sample_origin() -> Origin {
    Origin {
        forge: "github".to_owned(),
        host: "github.com".to_owned(),
        owner: "sample-owner".to_owned(),
        repository: "sample-owner/sample-project".to_owned(),
        id: "1".to_owned(),
        topics: vec!["sample-topic".to_owned()],
        properties: BTreeMap::from([("team".to_owned(), vec!["sample".to_owned()])]),
        visibility: Visibility::Public,
        source: Some(SourceName::new("sample-source")),
    }
}

/// The samples of the kind `spec` describes, each from the sample origin
/// where `origins` says every event a route takes has one, and each both
/// with it and without it otherwise.
#[must_use]
pub fn samples_from(spec: &KindSpec, origins: bool) -> Vec<Event> {
    let plain = samples(spec);
    let originated = plain
        .iter()
        .map(|event| event.clone().from_origin(sample_origin()));
    if origins {
        originated.collect()
    } else {
        originated.chain(plain.iter().cloned()).collect()
    }
}

/// The sample events of the kind `spec` describes, every template is
/// dry-rendered against: a plain one, a pre-release where the kind marks
/// one, one with a long body (ADR-0006), and one with no body where the
/// kind names a signal: a tag's (ADR-0031), or a package version's
/// (ADR-0035).
#[must_use]
pub fn samples(spec: &KindSpec) -> Vec<Event> {
    let plain = draft(spec);
    let prerelease = spec.attributes.contains_key(PRERELEASE).then(|| {
        let mut attributes = plain.attributes.clone();
        if let Some(Attribute::Text(version)) = attributes.get_mut(VERSION) {
            version.push_str("-rc.1");
        }
        attributes.insert(PRERELEASE.to_owned(), Attribute::Flag(true));
        Draft {
            attributes,
            ..plain.clone()
        }
    });
    let long = Draft {
        body: "- a sample change\n".repeat(LONG),
        ..plain.clone()
    };
    let tagged = spec.attributes.contains_key(SIGNAL).then(|| {
        let mut attributes = plain.attributes.clone();
        let signal = match signal_of(&spec.kind) {
            Signal::Release => Signal::Tag,
            other => other,
        };
        attributes.insert(SIGNAL.to_owned(), Attribute::Text(signal.name().to_owned()));
        Draft {
            attributes,
            body: String::new(),
            ..plain.clone()
        }
    });
    std::iter::once(plain)
        .chain(prerelease)
        .chain(std::iter::once(long))
        .chain(tagged)
        .filter_map(|draft| Event::normalise(draft).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use herald_core::KindSpecs;

    use super::*;

    #[test]
    fn a_sample_board_shows_a_person_s_pull_request_waiting_on_review_and_a_bot_s_bump() {
        let [full, empty] = sample_boards();
        assert_eq!(empty, Board::default());
        let row = &full.groups[0].rows[0];
        assert_eq!(full.groups[0].name, "waiting-on-review");
        assert_eq!(
            (
                row.pull.number,
                row.pull.title.as_str(),
                row.pull.url.as_str()
            ),
            (
                1,
                "Add a sample",
                "https://github.com/sample-owner/sample-project/pull/1"
            )
        );
        assert_eq!(row.pull.author.as_deref(), Some("sample-author"));
        assert_eq!(row.reason.written(), "review-requested");
        assert_eq!(row.waited, jiff::SignedDuration::from_hours(15 * 24));
        assert!(row.stale);
        let batch = &full.batches[0];
        let bump = batch.bump.as_ref().unwrap();
        assert_eq!(
            (bump.package.as_str(), bump.to.as_deref()),
            ("sample-package", Some("1.1.0"))
        );
        let bumped = &batch.rows[0].pull;
        assert_eq!(
            (bumped.number, bumped.url.as_str(), bumped.author.as_deref()),
            (
                2,
                "https://github.com/sample-owner/sample-project/pull/2",
                Some("dependabot[bot]")
            )
        );
        assert_eq!((batch.green, batch.failing), (1, 0));
        assert_eq!(batch.rows[0].reason.written(), "ready");
        assert_eq!(full.total(), 2);
    }

    #[test]
    fn a_versioned_kind_has_a_plain_a_pre_release_a_long_and_a_bodiless_sample() {
        let kinds = KindSpecs::default();
        for (kind, signal, bodiless) in [
            (Kind::RELEASE, Signal::Release, Signal::Tag),
            (Kind::PACKAGE_VERSION, Signal::Package, Signal::Package),
        ] {
            let samples = samples(kinds.get(&kind).unwrap());
            assert_eq!(samples.len(), 4, "{kind}");
            assert!(samples.iter().all(|event| event.kind() == &kind));
            assert!(samples[1].prerelease());
            assert!(samples[2].body().len() > 30_000);
            let signals: Vec<Option<Signal>> = samples.iter().map(Event::signal).collect();
            assert_eq!(
                signals,
                [Some(signal), Some(signal), Some(signal), Some(bodiless)]
            );
            assert_eq!(samples[3].body(), "");
        }
        let release = samples(kinds.get(&Kind::RELEASE).unwrap());
        assert_eq!(release[0].version(), Some("v1.2.0"));
        assert_eq!(release[1].version(), Some("v1.2.0-rc.1"));
    }

    #[test]
    fn samples_come_from_the_sample_origin_and_also_from_none_where_some_events_have_none() {
        let kinds = KindSpecs::default();
        let spec = kinds.get(&Kind::RELEASE).unwrap();
        let all = samples_from(spec, true);
        assert_eq!(all.len(), 4);
        assert!(
            all.iter()
                .all(|event| event.origin() == Some(&sample_origin()))
        );
        let mixed = samples_from(spec, false);
        assert_eq!(mixed.len(), 8);
        assert_eq!(
            mixed
                .iter()
                .filter(|event| event.origin().is_none())
                .count(),
            4
        );
        assert_eq!(sample_origin().checked(), Ok(sample_origin()));
    }

    #[test]
    fn a_kind_without_a_pre_release_has_a_plain_and_a_long_sample_with_every_attribute() {
        let kinds = KindSpecs::default();
        let entries = samples(kinds.get(&Kind::FEED_ENTRY).unwrap());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version(), None);
        assert_eq!(entries[0].link("entry"), Some("https://example.org/entry"));
        let declared = KindSpec {
            kind: Kind::new("deploy").unwrap(),
            description: "A deploy.".to_owned(),
            versioned: false,
            links: Vec::new(),
            attributes: [
                ("live", AttributeType::Flag),
                ("count", AttributeType::Number),
                ("site", AttributeType::Url),
                ("region", AttributeType::Text),
            ]
            .into_iter()
            .map(|(name, ty)| {
                (
                    name.to_owned(),
                    herald_core::AttributeSpec { ty, required: true },
                )
            })
            .collect(),
            private: false,
        };
        let deploy = &samples(&declared)[0];
        assert_eq!(deploy.attribute("live"), Some(&Attribute::Flag(false)));
        assert_eq!(
            deploy.attribute("count"),
            Some(&Attribute::Number(1.into()))
        );
        assert_eq!(
            deploy.attribute("site"),
            Some(&Attribute::Text("https://example.org/site".to_owned()))
        );
        assert_eq!(
            deploy.attribute("region"),
            Some(&Attribute::Text("sample region".to_owned()))
        );
    }
}
