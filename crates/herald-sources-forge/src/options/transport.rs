//! The keys that choose how an owner's members are polled: the transport,
//! GitHub's and GitLab's GraphQL batches, and Forgejo's feed (ADR-0030,
//! ADR-0037).

use std::num::NonZeroUsize;

use herald_core::{Address, Signal, web_url};

use super::SourceOptions;
use crate::Forge;

/// How many of an owner's members one GraphQL request asks for where
/// `graphql_batch` is absent.
pub const GRAPHQL_BATCH: NonZeroUsize = NonZeroUsize::new(50).unwrap();

/// The most `graphql_batch` may be: the most ids GitHub's GraphQL API
/// looks up in one request, and the most projects one GitLab page lists.
pub const GRAPHQL_BATCH_ALLOWED: usize = 100;

/// How many activities a page of a Forgejo feed asks for where
/// `feed_page` is absent: as many as an instance answers by default.
pub const FEED_PAGE: NonZeroUsize = NonZeroUsize::new(50).unwrap();

/// How many pages of a Forgejo feed a round reads back where `feed_pages`
/// is absent.
pub const FEED_PAGES: NonZeroUsize = NonZeroUsize::new(3).unwrap();

/// How an owner's members are polled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// On GitHub and GitLab, GraphQL in batches where the source has a
    /// token and REST per member where it has none; on Forgejo, REST.
    #[default]
    Auto,
    /// GraphQL in batches of `graphql_batch` members, on GitHub (which
    /// needs a token) and GitLab.
    Graphql,
    /// REST, one member at a time, each asked with its `ETag`.
    Rest,
    /// GitHub's owner events, read once a round, naming the members to
    /// poll, each through GraphQL where there is a token and REST
    /// otherwise.
    Events,
    /// A Forgejo owner's activity feed, read once a round, naming the
    /// members to poll, each through REST.
    Feed,
}

impl Transport {
    /// The transports `forge` offers.
    #[must_use]
    pub const fn offered(forge: Forge) -> &'static [Self] {
        match forge {
            Forge::GitHub => &[Self::Auto, Self::Graphql, Self::Rest, Self::Events],
            Forge::GitLab => &[Self::Auto, Self::Graphql, Self::Rest],
            Forge::Forgejo => &[Self::Auto, Self::Rest, Self::Feed],
        }
    }

    /// The transport as `herald.toml` writes it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Graphql => "graphql",
            Self::Rest => "rest",
            Self::Events => "events",
            Self::Feed => "feed",
        }
    }
}

impl SourceOptions {
    /// Whether `forge` offers the transport, GraphQL endpoint and batch
    /// the source names.
    pub(super) fn transport_check(&self, forge: Forge) -> Result<(), String> {
        if let Some(transport) = self
            .transport
            .filter(|transport| !Transport::offered(forge).contains(transport))
        {
            let offered: Vec<&str> = Transport::offered(forge)
                .iter()
                .map(|offered| offered.name())
                .collect();
            return Err(format!(
                "transport {} is not offered here; choose {}",
                transport.name(),
                offered.join(", ")
            ));
        }
        if forge == Forge::Forgejo
            && let Some(key) = [
                (self.graphql.is_some(), "graphql"),
                (self.graphql_batch.is_some(), "graphql_batch"),
            ]
            .into_iter()
            .find_map(|(given, key)| given.then_some(key))
        {
            return Err(format!(
                "{key} is a GitHub or GitLab owner's, as Forgejo has no GraphQL API; remove it"
            ));
        }
        if forge == Forge::GitLab && self.transport == Some(Transport::Graphql) && self.lists_more()
        {
            return Err(
                "transport graphql reads only Releases on GitLab, whose GraphQL API lists no tags or packages; choose rest or auto, or take only releases".to_owned(),
            );
        }
        if forge == Forge::GitHub
            && self.transport == Some(Transport::Graphql)
            && self.token.is_none()
        {
            return Err(
                "transport graphql needs a token, as GitHub's GraphQL API answers none without; give token, or choose rest".to_owned(),
            );
        }
        if let Some(graphql) = &self.graphql {
            if !web_url(graphql) {
                return Err(format!(
                    "graphql \"{graphql}\" is not a web address; give the GraphQL endpoint, such as https://ghe.example/api/graphql"
                ));
            }
            let origin = |url: &str| Address::parse(url).map(|address| address.origin());
            if origin(graphql) != origin(self.api.as_deref().unwrap_or(forge.api().url)) {
                return Err(format!(
                    "graphql \"{graphql}\" is not on api's host, the only one the source may reach; give the endpoint beside api, or remove graphql"
                ));
            }
        }
        if self.graphql_batch().get() > GRAPHQL_BATCH_ALLOWED {
            return Err(format!(
                "graphql_batch is at most {GRAPHQL_BATCH_ALLOWED}; lower it"
            ));
        }
        self.feed_check()
    }

    /// Whether `feed_page` and `feed_pages` are given only where the
    /// source reads a feed, and within four times their defaults.
    pub(super) fn feed_check(&self) -> Result<(), String> {
        let read = self.transport == Some(Transport::Feed);
        for (key, given, default) in [
            ("feed_page", self.feed_page, FEED_PAGE),
            ("feed_pages", self.feed_pages, FEED_PAGES),
        ] {
            let Some(given) = given else {
                continue;
            };
            if !read {
                return Err(format!(
                    "{key} is how a feed is read; set transport = \"feed\" on a Forgejo owner, or remove it"
                ));
            }
            let most = 4 * default.get();
            if given.get() > most {
                return Err(format!("{key} is at most {most}; lower it"));
            }
        }
        Ok(())
    }

    /// How many members one GraphQL request asks for.
    #[must_use]
    pub fn graphql_batch(&self) -> NonZeroUsize {
        self.graphql_batch.unwrap_or(GRAPHQL_BATCH)
    }

    /// How many activities one page of a Forgejo feed asks for.
    #[must_use]
    pub fn feed_page(&self) -> NonZeroUsize {
        self.feed_page.unwrap_or(FEED_PAGE)
    }

    /// How many pages of a Forgejo feed a round reads back.
    #[must_use]
    pub fn feed_pages(&self) -> NonZeroUsize {
        self.feed_pages.unwrap_or(FEED_PAGES)
    }

    /// How an owner's members are polled.
    #[must_use]
    pub fn transport(&self) -> Transport {
        self.transport.unwrap_or_default()
    }

    /// Whether a member may announce tags or packages, which GitLab's
    /// GraphQL API does not list.
    pub(super) fn lists_more(&self) -> bool {
        self.may_take(Signal::Tag) || self.may_take(Signal::Package)
    }

    /// Whether an owner's members on `forge` are polled together through
    /// GraphQL: as `transport` says, and under "auto" and "events" where
    /// there is a token; on GitLab only where no member takes tags or
    /// packages, and never on Forgejo, which has no GraphQL API.
    #[must_use]
    pub fn batched(&self, forge: Forge) -> bool {
        let offered = match forge {
            Forge::GitHub => true,
            Forge::GitLab => !self.lists_more(),
            Forge::Forgejo => false,
        };
        offered
            && match self.transport() {
                Transport::Graphql => true,
                Transport::Auto | Transport::Events => self.token.is_some(),
                Transport::Rest | Transport::Feed => false,
            }
    }
}
