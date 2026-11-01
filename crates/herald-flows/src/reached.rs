use herald_core::{Announcement, Delivery, Entry, Key, Marker, Profile, Sent, Step};
use herald_dest_discord::{Discord, DiscordMarkers};
use herald_ports::{Http, HttpError, Markers};

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

    /// What its own posts say landed for `key`, where it can read them back.
    pub(crate) async fn landed<H: Http>(
        &self,
        http: &H,
        key: &Key,
        marker: &Marker,
    ) -> Option<Entry> {
        match self {
            Self::Discord(discord) => DiscordMarkers::new(discord, http)
                .entry(key, marker)
                .await
                .ok()
                .flatten(),
        }
    }
}
