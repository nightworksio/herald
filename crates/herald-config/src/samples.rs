use herald_core::{Attribute, AttributeType, Draft, Event, KindSpec, PRERELEASE, VERSION};
use jiff::Timestamp;

/// Noon on 31 October 2026, when every sample happened.
const NOON: Timestamp = Timestamp::constant(1_793_448_000, 0);
/// The lines of a sample's long body, past every destination's limit.
const LONG: usize = 2_000;

fn draft(spec: &KindSpec) -> Draft {
    let attributes = spec
        .attributes
        .iter()
        .map(|(name, attribute)| {
            let sample = match attribute.ty {
                AttributeType::Text if name == VERSION => Attribute::Text("v1.2.0".to_owned()),
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

/// The sample events of the kind `spec` describes, every template is
/// dry-rendered against: a plain one, a pre-release where the kind marks
/// one, and one with a long body (ADR-0006).
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
    std::iter::once(plain)
        .chain(prerelease)
        .chain(std::iter::once(long))
        .filter_map(|draft| Event::normalise(draft).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use herald_core::{Kind, KindSpecs};

    use super::*;

    #[test]
    fn a_versioned_kind_has_a_plain_a_pre_release_and_a_long_sample() {
        let kinds = KindSpecs::default();
        for kind in [Kind::RELEASE, Kind::PACKAGE_VERSION] {
            let samples = samples(kinds.get(&kind).unwrap());
            assert_eq!(samples.len(), 3, "{kind}");
            assert!(samples.iter().all(|event| event.kind() == &kind));
            assert!(samples[1].prerelease());
            assert!(samples[2].body().len() > 30_000);
        }
        let release = samples(kinds.get(&Kind::RELEASE).unwrap());
        assert_eq!(release[0].version(), Some("v1.2.0"));
        assert_eq!(release[1].version(), Some("v1.2.0-rc.1"));
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
