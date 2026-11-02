use herald_core::{Announcement, Chore, Delivery, Entry, Key, Marker, Profile, Sent, Step};
use herald_dest_discord::{Discord, DiscordMarkers};
use herald_dest_fediverse::{Mastodon, MastodonMarkers};
use herald_dest_matrix::{Matrix, MatrixMarkers};
use herald_ports::{Http, HttpError, MarkedThread, Markers, MarkersError};

/// One route's destination, with its credential resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    /// Discord.
    Discord(Discord),
    /// An account through the Mastodon client API.
    Mastodon(Mastodon),
    /// A bot account on a Matrix homeserver.
    Matrix(Matrix),
}

impl Reached {
    /// What it can do.
    #[must_use]
    pub fn profile(&self) -> Profile {
        match self {
            Self::Discord(discord) => discord.profile(),
            Self::Mastodon(mastodon) => mastodon.profile(),
            Self::Matrix(matrix) => matrix.profile(),
        }
    }

    /// The base URL of everything it sends.
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::Discord(discord) => discord.api(),
            Self::Mastodon(mastodon) => mastodon.api(),
            Self::Matrix(matrix) => matrix.api(),
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
            Self::Mastodon(mastodon) => mastodon.send(http, step, announcement, entry).await,
            Self::Matrix(matrix) => matrix.send(http, step, announcement, entry).await,
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
            Self::Mastodon(_) => Ok(Sent::Refused(
                herald_dest_fediverse::Refusal::Cannot("archive or lock a thread").to_string(),
            )),
            Self::Matrix(_) => Ok(Sent::Refused(
                herald_dest_matrix::Refusal::Cannot("archive or lock a thread").to_string(),
            )),
        }
    }

    /// Its own posts, read through `http`.
    #[must_use]
    pub fn posts<'a, H: Http>(&'a self, http: &'a H) -> Posts<'a, H> {
        match self {
            Self::Discord(discord) => Posts::Discord(DiscordMarkers::new(discord, http)),
            Self::Mastodon(mastodon) => Posts::Mastodon(MastodonMarkers::new(mastodon, http)),
            Self::Matrix(matrix) => Posts::Matrix(MatrixMarkers::new(matrix, http)),
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
    /// A Mastodon account's.
    Mastodon(MastodonMarkers<'a, H>),
    /// A Matrix room's.
    Matrix(MatrixMarkers<'a, H>),
}

impl<H: Http> Markers for Posts<'_, H> {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.entry(key, marker).await,
            Self::Mastodon(posts) => posts.entry(key, marker).await,
            Self::Matrix(posts) => posts.entry(key, marker).await,
        }
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.threads(route).await,
            Self::Mastodon(posts) => posts.threads(route).await,
            Self::Matrix(posts) => posts.threads(route).await,
        }
    }
}
