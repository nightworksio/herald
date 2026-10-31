use std::collections::BTreeMap;
use std::fmt;

use jiff::Timestamp;
use serde::Serialize;

use crate::key::Identity;

/// What kind of thing an event announces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A release on a forge.
    Release,
    /// A new version of a package on a registry.
    PackageVersion,
    /// An entry in a feed.
    FeedEntry,
    /// Anything an app says.
    Message,
}

/// An event as a source parsed it, before [`Event::normalise`] checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// What makes this event this event.
    pub identity: String,
    /// What kind of thing it announces.
    pub kind: Kind,
    /// The project's name.
    pub project: String,
    /// The version, where the kind has one.
    pub version: Option<String>,
    /// A title, on one line.
    pub title: String,
    /// The body, in Markdown.
    pub body: String,
    /// Named links: `release`, `changelog`, `comparison`, `download`.
    pub links: BTreeMap<String, String>,
    /// Whether it is a pre-release, as the source says.
    pub prerelease: bool,
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
    version: Option<String>,
    title: String,
    body: String,
    links: BTreeMap<String, String>,
    prerelease: bool,
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
            version: draft
                .version
                .as_deref()
                .map(str::trim)
                .filter(|version| !version.is_empty())
                .map(ToOwned::to_owned),
            title,
            body: draft.body.trim().to_owned(),
            links,
            prerelease: draft.prerelease,
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
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// The project's name.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The version, where the event has one.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
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

    /// The first link, by name.
    #[must_use]
    pub fn first_link(&self) -> Option<&str> {
        self.links.values().next().map(String::as_str)
    }

    /// Whether it is a pre-release.
    #[must_use]
    pub fn prerelease(&self) -> bool {
        self.prerelease
    }

    /// When it happened.
    #[must_use]
    pub fn occurred(&self) -> Timestamp {
        self.occurred
    }
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
            if !(url.starts_with("https://") || url.starts_with("http://")) {
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
            kind: Kind::Release,
            project: "php-mutation-gate".to_owned(),
            version: Some("v0.1.0".to_owned()),
            title: "php-mutation-gate v0.1.0".to_owned(),
            body: "## Added\n\n- the gate".to_owned(),
            links: BTreeMap::from([(
                "release".to_owned(),
                "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0".to_owned(),
            )]),
            prerelease: false,
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
        assert_eq!(event.kind(), Kind::Release);
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
            version: Some(" 1.0 ".to_owned()),
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
        for version in [Some("  ".to_owned()), None] {
            let event = Event::normalise(Draft { version, ..draft() }).unwrap();
            assert_eq!(event.version(), None);
        }
    }

    #[test]
    fn the_prerelease_flag_is_kept() {
        let event = Event::normalise(Draft {
            prerelease: true,
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
                "version": "v0.1.0",
                "title": "php-mutation-gate v0.1.0",
                "body": "## Added\n\n- the gate",
                "links": {
                    "release": "https://github.com/nightworksio/php-mutation-gate/releases/tag/v0.1.0"
                },
                "prerelease": false,
                "occurred": "2026-10-31T12:00:00Z"
            })
        );
    }

    #[test]
    fn each_kind_serialises_in_kebab_case() {
        let kinds = [
            Kind::Release,
            Kind::PackageVersion,
            Kind::FeedEntry,
            Kind::Message,
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
