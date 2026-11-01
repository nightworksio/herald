use herald_core::{Announcement, Chore, Delivery, Entry, Key, Marker, Profile, Sent, Step};
use herald_dest_discord::{Discord, DiscordMarkers};
use herald_ports::{Http, HttpError, MarkedThread, Markers, MarkersError};

/// One route's destination, with its credential resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    /// Discord.
    Discord(Discord),
}

impl Reached {
    /// What it can do.
    #[must_use]
    pub fn profile(&self) -> Profile {
        match self {
            Self::Discord(discord) => discord.profile(),
        }
    }

    /// The base URL of everything it sends.
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::Discord(discord) => discord.api(),
        }
    }

    pub(crate) async fn send<H: Http>(
        &self,
        http: &H,
        step: &Step,
        announcement: &Announcement,
        delivery: &Delivery,
        entry: &Entry,
    ) -> Result<Sent, HttpError> {
        match self {
            Self::Discord(discord) => {
                discord
                    .send(http, step, announcement, delivery, entry)
                    .await
            }
        }
    }

    pub(crate) async fn housekeep<H: Http>(
        &self,
        http: &H,
        chore: Chore,
        thread: &str,
    ) -> Result<Sent, HttpError> {
        match self {
            Self::Discord(discord) => discord.housekeep(http, chore, thread).await,
        }
    }

    /// Its own posts, read through `http`.
    #[must_use]
    pub fn posts<'a, H: Http>(&'a self, http: &'a H) -> Posts<'a, H> {
        match self {
            Self::Discord(discord) => Posts::Discord(DiscordMarkers::new(discord, http)),
        }
    }

    /// What its own posts say landed for `key`, where it can read them back.
    pub(crate) async fn landed<H: Http>(
        &self,
        http: &H,
        key: &Key,
        marker: &Marker,
    ) -> Option<Entry> {
        self.posts(http).entry(key, marker).await.ok().flatten()
    }
}

/// One route's destination's own posts, read back for their markers.
#[derive(Debug)]
pub enum Posts<'a, H> {
    /// A Discord channel's.
    Discord(DiscordMarkers<'a, H>),
}

impl<H: Http> Markers for Posts<'_, H> {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.entry(key, marker).await,
        }
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.threads(route).await,
        }
    }
}
