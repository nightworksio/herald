use jiff::SignedDuration;

use crate::event::{Event, Kind};
use crate::key::RouteId;

macro_rules! name {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// The name as the config writes it.
            #[must_use]
            pub fn new(name: impl Into<String>) -> Self {
                Self(name.into())
            }

            /// The name as written.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

name!(
    /// The name a config gives a source.
    SourceName
);
name!(
    /// The name a config gives a destination.
    DestinationName
);
name!(
    /// The name a config gives a template.
    TemplateName
);
name!(
    /// A role a route mentions, by the destination's id for it.
    RoleId
);

/// A secret, by the name the platform keeps it under: `{ secret = "NAME" }`.
/// The config holds this and never the secret's value (ADR-0007).
#[derive(Debug, Clone, PartialEq, Eq, Hash, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct SecretRef {
    /// The secret's name.
    pub secret: String,
}

/// How a secret is written, for an error that never repeats what was written
/// in its place.
const SECRET_FORM: &str = "a secret is written as { secret = \"NAME\" }, naming where the platform keeps it, never as its value";

struct SecretVisitor;

impl<'de> serde::de::Visitor<'de> for SecretVisitor {
    type Value = SecretRef;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(SECRET_FORM)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<SecretRef, A::Error> {
        let mut secret = None;
        while let Some(key) = map.next_key::<String>()? {
            if key != "secret" || secret.is_some() {
                return Err(serde::de::Error::custom(SECRET_FORM));
            }
            secret = Some(map.next_value::<String>()?);
        }
        secret
            .filter(|name| !name.trim().is_empty())
            .map(|secret| SecretRef { secret })
            .ok_or_else(|| serde::de::Error::custom(SECRET_FORM))
    }
}

impl<'de> serde::Deserialize<'de> for SecretRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer
            .deserialize_map(SecretVisitor)
            .map_err(|_: D::Error| serde::de::Error::custom(SECRET_FORM))
    }
}

/// Which pre-releases a route takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Prereleases {
    /// Releases and pre-releases alike.
    #[default]
    Include,
    /// Releases only.
    Exclude,
    /// Pre-releases only.
    Only,
}

/// The events a route takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// The sources it takes events from.
    pub sources: Vec<SourceName>,
    /// The kinds it takes; none means every kind.
    pub kinds: Vec<Kind>,
    /// Which pre-releases it takes.
    pub prereleases: Prereleases,
}

impl Filter {
    /// Whether the filter takes `event`, which came from `source`.
    #[must_use]
    pub fn takes(&self, source: &SourceName, event: &Event) -> bool {
        let prerelease = match self.prereleases {
            Prereleases::Include => true,
            Prereleases::Exclude => !event.prerelease(),
            Prereleases::Only => event.prerelease(),
        };
        self.sources.contains(source)
            && (self.kinds.is_empty() || self.kinds.contains(&event.kind()))
            && prerelease
    }
}

/// The thread a route opens for each announcement, and when housekeeping
/// closes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// The template of the thread's name.
    pub name: String,
    /// How long after the announcement the thread is archived.
    pub archive_after: Option<SignedDuration>,
    /// How long after the announcement the thread is locked.
    pub lock_after: Option<SignedDuration>,
}

/// Who a route mentions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mentions {
    /// The roles it pings, where the destination has roles.
    pub roles: Vec<RoleId>,
    /// Whether it mentions the subscribers herald keeps, where the destination
    /// keeps none of its own.
    pub subscribers: bool,
}

/// How a route keeps its newest announcement pinned: each pin unpins the
/// route's earlier one, found as this says (ADR-0011).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Pinning {
    /// Among the destination's pins, by the route marker herald's own posts
    /// carry. Every store serves it.
    #[default]
    Markers,
    /// By the pinned message the store remembers for the route. Only a store
    /// that keeps pins serves it.
    Store,
}

/// How a route delivers, in the features every destination describes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Delivery {
    /// Whether to crosspost the announcement.
    pub crosspost: bool,
    /// The thread to open for it.
    pub thread: Option<Thread>,
    /// Whether to pin it, unpinning the route's earlier pin, and how that
    /// pin is found.
    pub pin: Option<Pinning>,
    /// Who to mention.
    pub mentions: Mentions,
}

/// An event filter leading to a destination, a template and how to deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The route's stable id.
    pub id: RouteId,
    /// The events it takes.
    pub filter: Filter,
    /// Where it announces them.
    pub destination: DestinationName,
    /// The template it renders them with.
    pub template: TemplateName,
    /// How it delivers them.
    pub delivery: Delivery,
}

/// Every route of `routes` that takes `event` from `source`, in the order the
/// routes are given.
#[must_use]
pub fn route<'a>(source: &SourceName, event: &Event, routes: &'a [Route]) -> Vec<&'a Route> {
    routes
        .iter()
        .filter(|route| route.filter.takes(source, event))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::event::tests::{draft, event};
    use crate::event::{Draft, Event};

    pub(crate) fn route_for(id: &str, source: &str) -> Route {
        Route {
            id: RouteId::new(id),
            filter: Filter {
                sources: vec![SourceName::new(source)],
                kinds: vec![],
                prereleases: Prereleases::Include,
            },
            destination: DestinationName::new("discord"),
            template: TemplateName::new("package-release"),
            delivery: Delivery::default(),
        }
    }

    fn prerelease() -> Event {
        Event::normalise(Draft {
            prerelease: true,
            ..draft()
        })
        .unwrap()
    }

    fn source() -> SourceName {
        SourceName::new("php-mutation-gate")
    }

    #[test]
    fn a_route_takes_events_from_its_sources_only() {
        let route = route_for("r", "php-mutation-gate");
        assert!(route.filter.takes(&source(), &event()));
        assert!(!route.filter.takes(&SourceName::new("herald"), &event()));
    }

    #[test]
    fn a_route_with_kinds_takes_those_kinds_only() {
        let mut route = route_for("r", "php-mutation-gate");
        route.filter.kinds = vec![Kind::PackageVersion];
        assert!(!route.filter.takes(&source(), &event()));
        route.filter.kinds = vec![Kind::PackageVersion, Kind::Release];
        assert!(route.filter.takes(&source(), &event()));
    }

    #[test]
    fn a_route_takes_the_prereleases_it_asks_for() {
        let mut route = route_for("r", "php-mutation-gate");
        let cases = [
            (Prereleases::Include, true, true),
            (Prereleases::Exclude, true, false),
            (Prereleases::Only, false, true),
        ];
        for (prereleases, release, pre) in cases {
            route.filter.prereleases = prereleases;
            assert_eq!(
                route.filter.takes(&source(), &event()),
                release,
                "{prereleases:?}"
            );
            assert_eq!(
                route.filter.takes(&source(), &prerelease()),
                pre,
                "{prereleases:?}"
            );
        }
    }

    #[test]
    fn a_secret_is_read_by_its_name_and_never_from_its_value() {
        let named: SecretRef =
            serde_json::from_str(r#"{"secret":"HERALD_DISCORD_TOKEN"}"#).unwrap();
        assert_eq!(named.secret, "HERALD_DISCORD_TOKEN");
        for written in [
            r#""xoxb-not-a-name""#,
            r#"{"secret":""}"#,
            r#"{"secret":"A","secret":"B"}"#,
            r#"{"value":"xoxb-not-a-name"}"#,
            r#"{"secret":7}"#,
            "{}",
            "[]",
        ] {
            let error = serde_json::from_str::<SecretRef>(written)
                .unwrap_err()
                .to_string();
            assert!(error.starts_with(SECRET_FORM), "{error}");
            assert!(!error.contains("xoxb"), "{error}");
        }
    }

    #[test]
    fn prereleases_are_included_by_default() {
        assert_eq!(Prereleases::default(), Prereleases::Include);
    }

    #[test]
    fn routing_keeps_every_taking_route_in_order() {
        let routes = [
            route_for("first", "php-mutation-gate"),
            route_for("elsewhere", "herald"),
            route_for("second", "php-mutation-gate"),
        ];
        let ids: Vec<&str> = route(&source(), &event(), &routes)
            .iter()
            .map(|route| route.id.as_str())
            .collect();
        assert_eq!(ids, ["first", "second"]);
    }

    #[test]
    fn names_keep_what_they_were_given() {
        assert_eq!(SourceName::new("s").as_str(), "s");
        assert_eq!(DestinationName::new("d").as_str(), "d");
        assert_eq!(TemplateName::new("t").as_str(), "t");
        assert_eq!(RoleId::new("1").as_str(), "1");
    }
}
