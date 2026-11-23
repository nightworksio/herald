use std::collections::BTreeMap;
use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::key::Identity;
use crate::kind::{Attribute, Kind, prerelease_in, version_in};
use crate::members::Visibility;
use crate::origin::Origin;
use crate::queue::Printed;
use crate::route::SourceName;

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
    /// A field of the origin is over its length.
    OriginTooLong,
    /// The origin's forge is not a kind's name.
    OriginForge,
    /// The origin's host is not a host.
    OriginHost,
    /// The origin's owner or repository is not a path of names.
    OriginPath,
    /// The origin has no id.
    OriginId,
    /// The origin names more areas than a source's table may.
    OriginAreas,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoIdentity => "the event has no identity; a source gives every event one",
            Self::NoProject => "the event names no project",
            Self::NoTitle => "the event has no title",
            Self::UnnamedLink => "a link of the event has no name",
            Self::LinkNotWeb => "a link of the event is not an http or https URL",
            Self::OriginTooLong => "a field of the event's origin is longer than 512 bytes",
            Self::OriginForge => {
                "the event's origin.forge is not a kind's name, of lowercase letters, digits and '-'"
            }
            Self::OriginHost => "the event's origin.host is not a host, such as forge.example:3000",
            Self::OriginPath => {
                "the event's origin.owner or origin.repository is not a path of names joined by '/'"
            }
            Self::OriginId => "the event's origin has no id",
            Self::OriginAreas => "the event's origin.areas names more areas than a source may",
        })
    }
}

/// A source-neutral event, checked and ready to route; read back from its
/// JSON only where it is still one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Written")]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    visibility: Option<Visibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<Box<Origin>>,
}

/// An event as its JSON holds it, checked again as it is read back.
#[derive(Deserialize)]
struct Written {
    identity: String,
    kind: Kind,
    project: String,
    title: String,
    body: String,
    links: BTreeMap<String, String>,
    attributes: BTreeMap<String, Attribute>,
    occurred: Timestamp,
    #[serde(default)]
    visibility: Option<Visibility>,
    #[serde(default)]
    origin: Option<Box<Origin>>,
}

impl TryFrom<Written> for Event {
    type Error = Problem;

    fn try_from(written: Written) -> Result<Self, Problem> {
        let visibility = written.visibility;
        let origin = written.origin;
        Self::normalise(Draft {
            identity: written.identity,
            kind: written.kind,
            project: written.project,
            title: written.title,
            body: written.body,
            links: written.links,
            attributes: written.attributes,
            occurred: written.occurred,
        })
        .map(|event| Self {
            visibility,
            origin,
            ..event
        })
    }
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
            visibility: None,
            origin: None,
        })
    }

    /// The event, seen only by those `visibility` lets see it, whatever the
    /// repository it is of: a package's own visibility (ADR-0035). Where it
    /// already names who may see it, the stricter of the two holds, so a
    /// later bound never loosens an earlier one.
    #[must_use]
    pub const fn seen_as(mut self, visibility: Visibility) -> Self {
        self.visibility = Some(match self.visibility {
            Some(own) => own.stricter(visibility),
            None => visibility,
        });
        self
    }

    /// Who may see it, where its source names that apart from its
    /// repository.
    #[must_use]
    pub const fn visibility(&self) -> Option<Visibility> {
        self.visibility
    }

    /// Who may see it: its own visibility where its source names one, and
    /// its origin's otherwise, where it has an origin.
    #[must_use]
    pub fn bounded_by(&self) -> Option<Visibility> {
        self.visibility
            .or_else(|| self.origin.as_ref().map(|origin| origin.visibility))
    }

    /// The event, from `origin`.
    #[must_use]
    pub fn from_origin(self, origin: Origin) -> Self {
        Self {
            origin: Some(Box::new(origin)),
            ..self
        }
    }

    /// Where it comes from, where its source says.
    #[must_use]
    pub fn origin(&self) -> Option<&Origin> {
        self.origin.as_deref()
    }

    /// The source it reached herald through, where its origin names one.
    #[must_use]
    pub fn source(&self) -> Option<&SourceName> {
        self.origin().and_then(|origin| origin.source.as_ref())
    }

    /// The event, its origin naming `source` as what it reached herald
    /// through, where it has an origin.
    #[must_use]
    pub fn through(mut self, source: &SourceName) -> Self {
        if let Some(origin) = &mut self.origin {
            origin.source = Some(source.clone());
        }
        self
    }

    /// The event, announced as of the project `project`.
    ///
    /// # Errors
    ///
    /// When `project` is empty once trimmed.
    pub fn of_project(self, project: &str) -> Result<Self, Problem> {
        let project = project.trim();
        if project.is_empty() {
            return Err(Problem::NoProject);
        }
        Ok(Self {
            project: project.to_owned(),
            ..self
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
        version_in(&self.attributes)
    }

    /// What output that may be read in public, such as a CI job's or a
    /// server's log, names it by: its version, else its title, where it is
    /// public; an event of a private or internal repository is named only
    /// as private (ADR-0067).
    #[must_use]
    pub fn named(&self) -> &str {
        self.named_in(Printed::Withheld)
    }

    /// What a command's output, as private as `printed` says, names it by:
    /// as [`Event::named`] does, but by its version or title wherever the
    /// operator said the output is private (ADR-0080).
    #[must_use]
    pub fn named_in(&self, printed: Printed) -> &str {
        match self.bounded_by() {
            Some(visibility) if !printed.prints(visibility) => Self::PRIVATELY_NAMED,
            Some(_) | None => self.version().unwrap_or(&self.title),
        }
    }

    /// What [`Event::named`] names an event of a private or internal
    /// repository by.
    pub const PRIVATELY_NAMED: &str = "(private)";

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

    /// The signal it is of, where its source names one.
    #[must_use]
    pub fn signal(&self) -> Option<crate::Signal> {
        match self.attributes.get(crate::SIGNAL) {
            Some(Attribute::Text(name)) => crate::Signal::named(name),
            _ => None,
        }
    }

    /// Whether it is an edit of an event the forge published before.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.attributes.get(crate::EDITED) == Some(&Attribute::Flag(true))
    }

    /// Whether it is the forge's deletion of an event it published before.
    #[must_use]
    pub fn withdrawn(&self) -> bool {
        self.attributes.get(crate::WITHDRAWN) == Some(&Attribute::Flag(true))
    }

    /// It, announced under `identity` in place of its own, as a release
    /// re-created after its withdrawal is (ADR-0048).
    #[must_use]
    pub fn reissued(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// It, marked as the forge's deletion of an event it published before.
    #[must_use]
    pub fn marked_withdrawn(mut self) -> Self {
        self.attributes
            .insert(crate::WITHDRAWN.to_owned(), Attribute::Flag(true));
        self
    }

    /// It, marked as an edit of an event the forge published before.
    #[must_use]
    pub fn marked_edited(mut self) -> Self {
        self.attributes
            .insert(crate::EDITED.to_owned(), Attribute::Flag(true));
        self
    }

    /// Whether it is a pre-release.
    #[must_use]
    pub fn prerelease(&self) -> bool {
        prerelease_in(&self.attributes)
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
    fn output_names_a_public_event_by_its_version_or_title_and_a_private_one_only_as_private() {
        assert_eq!(event().named(), "v0.1.0");
        assert_eq!(event().seen_as(Visibility::Public).named(), "v0.1.0");
        let untitled = Event::normalise(Draft {
            attributes: BTreeMap::new(),
            kind: Kind::FEED_ENTRY,
            ..draft()
        })
        .unwrap();
        assert_eq!(untitled.named(), "php-mutation-gate v0.1.0");
        for hidden in [Visibility::Private, Visibility::Internal] {
            assert_eq!(event().seen_as(hidden).named(), Event::PRIVATELY_NAMED);
            let hidden = event().seen_as(hidden);
            assert_eq!(hidden.named_in(Printed::Withheld), Event::PRIVATELY_NAMED);
            assert_eq!(hidden.named_in(Printed::Configured), "v0.1.0");
            assert_eq!(hidden.named_in(Printed::Asked), "v0.1.0");
        }
        assert_eq!(event().named_in(Printed::Withheld), "v0.1.0");
    }

    #[test]
    fn an_event_named_for_another_project_keeps_everything_else() {
        let renamed = event().of_project("  plugins/gate ").unwrap();
        assert_eq!(renamed.project(), "plugins/gate");
        assert_eq!(renamed.identity(), event().identity());
        assert_eq!(renamed.title(), event().title());
        assert_eq!(event().of_project(" \t"), Err(Problem::NoProject));
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
    fn the_signal_is_read_from_its_attribute_where_it_names_one() {
        let signalled = |value: Option<Attribute>| {
            let mut attributes = crate::versioned("v1", false);
            if let Some(value) = value {
                attributes.insert(crate::SIGNAL.to_owned(), value);
            }
            Event::normalise(Draft {
                attributes,
                ..draft()
            })
            .unwrap()
            .signal()
        };
        assert_eq!(
            signalled(Some(Attribute::Text("tag".to_owned()))),
            Some(crate::Signal::Tag)
        );
        assert_eq!(
            signalled(Some(Attribute::Text("release".to_owned()))),
            Some(crate::Signal::Release)
        );
        assert_eq!(signalled(Some(Attribute::Text("other".to_owned()))), None);
        assert_eq!(signalled(Some(Attribute::Flag(true))), None);
        assert_eq!(signalled(None), None);
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
    fn an_event_reads_back_from_its_json_only_where_it_is_still_one() {
        let json = serde_json::to_string(&event()).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), event());
        let untitled = json.replace("\"title\":\"php-mutation-gate v0.1.0\"", "\"title\":\" \"");
        let error = serde_json::from_str::<Event>(&untitled)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with(&Problem::NoTitle.to_string()), "{error}");
        assert!(!json.contains("visibility"));
        assert_eq!(event().visibility(), None);
        let private = event().seen_as(Visibility::Private);
        assert_eq!(private.visibility(), Some(Visibility::Private));
        let json = serde_json::to_string(&private).unwrap();
        assert!(json.contains("\"visibility\":\"private\""), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), private);
    }

    #[test]
    fn a_later_bound_only_ever_tightens_who_may_see_an_event() {
        let all = [
            Visibility::Public,
            Visibility::Internal,
            Visibility::Private,
        ];
        for first in all {
            for then in all {
                let seen = event().seen_as(first).seen_as(then).visibility();
                assert_eq!(seen, Some(first.max(then)), "{first:?} then {then:?}");
                assert_eq!(first.stricter(then), first.max(then));
            }
        }
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
