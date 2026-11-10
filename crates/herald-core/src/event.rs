use std::collections::BTreeMap;
use std::fmt;

use jiff::Timestamp;
use serde::Serialize;

use crate::key::Identity;
use crate::kind::{Attribute, Kind, PRERELEASE, VERSION};

/// An event as a source parsed it, before [`Event::normalise`] checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// What makes this event this event.
    pub identity: String,
    /// What kind of thing it announces.
    pub kind: Kind,
    /// The project's name.
    pub project: String,
    /// A title, on one line.
    pub title: String,
    /// The body, in Markdown.
    pub body: String,
    /// Named links: `release`, `changelog`, `comparison`, `download`.
    pub links: BTreeMap<String, String>,
    /// Its kind's attributes, by name: a versioned kind's `version`, and
    /// `prerelease`.
    pub attributes: BTreeMap<String, Attribute>,
    /// When it happened, as the source says.
    pub occurred: Timestamp,
}

/// A field of a [`Draft`] that cannot become an [`Event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The identity is empty.
    NoIdentity,
    /// The project is empty.
    NoProject,
    /// The title is empty.
    NoTitle,
    /// A link's name is empty.
    UnnamedLink,
    /// A link is not an `https` or `http` URL.
    LinkNotWeb,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoIdentity => "the event has no identity; a source gives every event one",
            Self::NoProject => "the event names no project",
            Self::NoTitle => "the event has no title",
            Self::UnnamedLink => "a link of the event has no name",
            Self::LinkNotWeb => "a link of the event is not an http or https URL",
        })
    }
}

/// A source-neutral event, checked and ready to route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    #[serde(serialize_with = "identity_text")]
    identity: Identity,
    kind: Kind,
    project: String,
    title: String,
    body: String,
    links: BTreeMap<String, String>,
    attributes: BTreeMap<String, Attribute>,
    occurred: Timestamp,
}

fn identity_text<S: serde::Serializer>(
    identity: &Identity,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(identity.as_str())
}

impl Event {
    /// The event `draft` describes, with its text trimmed and its title on one
    /// line, or the first problem that keeps it from being one.
    ///
    /// # Errors
    ///
    /// The [`Problem`] of the first field that cannot be part of an event.
    pub fn normalise(draft: Draft) -> Result<Self, Problem> {
        let identity = draft.identity.trim();
        let project = draft.project.trim();
        let title = draft.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if identity.is_empty() {
            return Err(Problem::NoIdentity);
        }
        if project.is_empty() {
            return Err(Problem::NoProject);
        }
        if title.is_empty() {
            return Err(Problem::NoTitle);
        }
        let links = links(draft.links)?;
        Ok(Self {
            identity: Identity::new(identity),
            kind: draft.kind,
            project: project.to_owned(),
            title,
            body: draft.body.trim().to_owned(),
            links,
            attributes: attributes(draft.attributes),
            occurred: draft.occurred,
        })
    }

    /// What makes this event this event.
    #[must_use]
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// What kind of thing it announces.
    #[must_use]
    pub const fn kind(&self) -> &Kind {
        &self.kind
    }

    /// The project's name.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The version, where its kind carries one.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        match self.attributes.get(VERSION) {
            Some(Attribute::Text(version)) => Some(version),
            _ => None,
        }
    }

    /// The attribute named `name`.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.get(name)
    }

    /// Every attribute, by name.
    #[must_use]
    pub const fn attributes(&self) -> &BTreeMap<String, Attribute> {
        &self.attributes
    }

    /// The title, on one line.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The body, in Markdown.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The link named `name`.
    #[must_use]
    pub fn link(&self, name: &str) -> Option<&str> {
        self.links.get(name).map(String::as_str)
    }

    /// Every link, by name.
    #[must_use]
    pub fn links(&self) -> &BTreeMap<String, String> {
        &self.links
    }

    /// The first link, by name.
    #[must_use]
    pub fn first_link(&self) -> Option<&str> {
        self.links.values().next().map(String::as_str)
    }

    /// Whether it is a pre-release.
    #[must_use]
    pub fn prerelease(&self) -> bool {
        self.attributes.get(PRERELEASE) == Some(&Attribute::Flag(true))
    }

    /// When it happened.
    #[must_use]
    pub fn occurred(&self) -> Timestamp {
        self.occurred
    }
}

/// `attributes`, their text trimmed and each that is left empty dropped.
fn attributes(attributes: BTreeMap<String, Attribute>) -> BTreeMap<String, Attribute> {
    attributes
        .into_iter()
        .filter_map(|(name, attribute)| match attribute {
            Attribute::Text(text) => {
                let text = text.trim();
                (!text.is_empty()).then(|| (name, Attribute::Text(text.to_owned())))
            }
            other => Some((name, other)),
        })
        .collect()
}

fn links(links: BTreeMap<String, String>) -> Result<BTreeMap<String, String>, Problem> {
    links
        .into_iter()
        .map(|(name, url)| {
            let name = name.trim().to_owned();
            let url = url.trim().to_owned();
            if name.is_empty() {
                return Err(Problem::UnnamedLink);
            }
            if !crate::address::web_url(&url) {
                return Err(Problem::LinkNotWeb);
            }
            Ok((name, url))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn draft() -> Draft {
        Draft {
            identity: "github.com/1066469473/v0.1.0".to_owned(),
            kind: Kind::RELEASE,
            project: "php-mutation-gate".to_owned(),
            attributes: crate::versioned("v0.1.0", false),
            title: "php-mutation-gate v0.1.0".to_owned(),
            body: "## Added\n\n- the gate".to_owned(),
            links: BTreeMap::from([(
                "release".to_owned(),
                "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0".to_owned(),
            )]),
            occurred: "2026-10-31T12:00:00Z".parse().unwrap(),
        }
    }

    pub(crate) fn event() -> Event {
        Event::normalise(draft()).unwrap()
    }

    #[test]
    fn a_draft_becomes_an_event_with_its_fields() {
        let event = event();
        assert_eq!(event.identity().as_str(), "github.com/1066469473/v0.1.0");
        assert_eq!(event.kind(), &Kind::RELEASE);
        assert_eq!(event.project(), "php-mutation-gate");
        assert_eq!(event.version(), Some("v0.1.0"));
        assert_eq!(event.title(), "php-mutation-gate v0.1.0");
        assert_eq!(event.body(), "## Added\n\n- the gate");
        assert_eq!(
            event.link("release"),
            Some("https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0")
        );
        assert_eq!(event.link("changelog"), None);
        assert!(!event.prerelease());
        assert_eq!(event.occurred().to_string(), "2026-10-31T12:00:00Z");
    }

    #[test]
    fn text_is_trimmed_and_the_title_kept_on_one_line() {
        let event = Event::normalise(Draft {
            identity: " id ".to_owned(),
            project: " project ".to_owned(),
            title: "  a\ttitle\n over\r\nlines ".to_owned(),
            attributes: crate::versioned(" 1.0 ", false),
            body: "\n body \n".to_owned(),
            links: BTreeMap::from([(" release ".to_owned(), " https://example.org ".to_owned())]),
            ..draft()
        })
        .unwrap();
        assert_eq!(event.identity().as_str(), "id");
        assert_eq!(event.project(), "project");
        assert_eq!(event.title(), "a title over lines");
        assert_eq!(event.version(), Some("1.0"));
        assert_eq!(event.body(), "body");
        assert_eq!(event.link("release"), Some("https://example.org"));
    }

    #[test]
    fn a_blank_or_missing_version_is_no_version() {
        for attributes in [crate::versioned("  ", false), BTreeMap::new()] {
            let event = Event::normalise(Draft {
                attributes,
                ..draft()
            })
            .unwrap();
            assert_eq!(event.version(), None);
        }
    }

    #[test]
    fn the_prerelease_flag_is_kept() {
        let event = Event::normalise(Draft {
            attributes: crate::versioned("v0.1.0-rc.1", true),
            ..draft()
        })
        .unwrap();
        assert!(event.prerelease());
    }

    #[test]
    fn a_blank_required_field_is_named() {
        let cases = [
            (
                Draft {
                    identity: " ".to_owned(),
                    ..draft()
                },
                Problem::NoIdentity,
            ),
            (
                Draft {
                    project: "\n".to_owned(),
                    ..draft()
                },
                Problem::NoProject,
            ),
            (
                Draft {
                    title: "\t".to_owned(),
                    ..draft()
                },
                Problem::NoTitle,
            ),
        ];
        for (draft, problem) in cases {
            assert_eq!(Event::normalise(draft), Err(problem));
        }
    }

    #[test]
    fn a_link_must_be_named_and_point_at_the_web() {
        let link = |name: &str, url: &str| Draft {
            links: BTreeMap::from([(name.to_owned(), url.to_owned())]),
            ..draft()
        };
        assert_eq!(
            Event::normalise(link(" ", "https://example.org")),
            Err(Problem::UnnamedLink)
        );
        assert_eq!(
            Event::normalise(link("release", "javascript:alert(1)")),
            Err(Problem::LinkNotWeb)
        );
        assert_eq!(
            Event::normalise(link("release", "ftp://example.org")),
            Err(Problem::LinkNotWeb)
        );
        assert!(Event::normalise(link("release", "http://example.org")).is_ok());
    }

    #[test]
    fn every_problem_reads_as_a_sentence_about_the_event() {
        for problem in [
            Problem::NoIdentity,
            Problem::NoProject,
            Problem::NoTitle,
            Problem::UnnamedLink,
            Problem::LinkNotWeb,
        ] {
            assert!(problem.to_string().contains("event"), "{problem}");
        }
    }

    #[test]
    fn an_event_serialises_as_templates_read_it() {
        let value = serde_json::to_value(event()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "identity": "github.com/1066469473/v0.1.0",
                "kind": "release",
                "project": "php-mutation-gate",
                "title": "php-mutation-gate v0.1.0",
                "body": "## Added\n\n- the gate",
                "links": {
                    "release": "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0"
                },
                "attributes": {"version": "v0.1.0", "prerelease": false},
                "occurred": "2026-10-31T12:00:00Z"
            })
        );
    }

    #[test]
    fn each_kind_serialises_in_kebab_case() {
        let kinds = [
            Kind::RELEASE,
            Kind::PACKAGE_VERSION,
            Kind::FEED_ENTRY,
            Kind::MESSAGE,
        ];
        let names: Vec<String> = kinds
            .iter()
            .map(|kind| serde_json::to_string(kind).unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "\"release\"",
                "\"package-version\"",
                "\"feed-entry\"",
                "\"message\""
            ]
        );
    }
}
