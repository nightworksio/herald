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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    /// The secret's name.
    pub secret: String,
}

/// Which pre-releases a route takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// How a route delivers, in the features every destination describes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Delivery {
    /// Whether to crosspost the announcement.
    pub crosspost: bool,
    /// The thread to open for it.
    pub thread: Option<Thread>,
    /// Whether to pin it.
    pub pin: bool,
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
