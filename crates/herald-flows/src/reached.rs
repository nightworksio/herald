use herald_core::{Announcement, Chore, Delivery, Entry, Key, Marker, Profile, Sent, Step};
use herald_dest_discord::{Discord, DiscordMarkers};
use herald_dest_fediverse::{ActivityPub, Mastodon, MastodonMarkers};
use herald_dest_matrix::{Matrix, MatrixMarkers};
use herald_dest_slack::{Slack, SlackMarkers};
use herald_ports::{Http, HttpError, MarkedThread, Markers, MarkersError, Signer};
use jiff::Timestamp;

/// One route's destination, with its credential resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reached {
    /// Discord.
    Discord(Discord),
    /// An account through the Mastodon client API.
    Mastodon(Mastodon),
    /// A bot account on a Matrix homeserver.
    Matrix(Matrix),
    /// Slack, as a bot or through a webhook.
    Slack(Slack),
    /// herald's own ActivityPub actor.
    ActivityPub(ActivityPub),
}

/// What a step is signed with, where its destination signs its requests.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Signing<'a, G> {
    pub(crate) signer: &'a G,
    pub(crate) now: Timestamp,
}

impl Reached {
    /// The actor whose followers each announcement is delivered to, where
    /// the destination delivers to followers.
    #[must_use]
    pub fn follows(&self) -> Option<&str> {
        match self {
            Self::ActivityPub(actor) => Some(actor.actor()),
            _ => None,
        }
    }

    /// The document `announcement` is published as where the destination is
    /// herald's own and serves it: an ActivityPub actor's Note, beside the
    /// actor whose outbox lists it.
    #[must_use]
    pub fn published(&self, announcement: &Announcement) -> Option<(&str, String)> {
        match self {
            Self::ActivityPub(actor) => Some((actor.actor(), actor.note(announcement))),
            _ => None,
        }
    }

    /// Whether `follower` may be delivered to: by an ActivityPub actor's
    /// policy, and by every other destination, which follows no one.
    #[must_use]
    pub fn admits(&self, follower: &str) -> bool {
        match self {
            Self::ActivityPub(actor) => actor.policy().admits(follower),
            _ => true,
        }
    }

    /// What it can do.
    #[must_use]
    pub fn profile(&self) -> Profile {
        match self {
            Self::Discord(discord) => discord.profile(),
            Self::Mastodon(mastodon) => mastodon.profile(),
            Self::Matrix(matrix) => matrix.profile(),
            Self::Slack(slack) => slack.profile(),
            Self::ActivityPub(actor) => actor.profile(),
        }
    }

    /// The base URL of everything it sends.
    #[must_use]
    pub fn api(&self) -> &str {
        match self {
            Self::Discord(discord) => discord.api(),
            Self::Mastodon(mastodon) => mastodon.api(),
            Self::Matrix(matrix) => matrix.api(),
            Self::Slack(slack) => slack.api(),
            Self::ActivityPub(actor) => actor.api(),
        }
    }

    pub(crate) async fn send<H: Http, G: Signer>(
        &self,
        http: &H,
        signing: Signing<'_, G>,
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
            Self::Slack(slack) => slack.send(http, step, announcement, entry).await,
            Self::ActivityPub(actor) => {
                actor
                    .send(http, signing.signer, signing.now, step, announcement, entry)
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
            Self::Mastodon(_) => Ok(Sent::Refused(
                herald_dest_fediverse::Refusal::Cannot("archive or lock a thread").to_string(),
            )),
            Self::Matrix(_) => Ok(Sent::Refused(
                herald_dest_matrix::Refusal::Cannot("archive or lock a thread").to_string(),
            )),
            Self::Slack(_) => Ok(Sent::Refused(
                herald_dest_slack::Refusal::Cannot("archive or lock a thread").to_string(),
            )),
            Self::ActivityPub(_) => Ok(Sent::Refused(
                "herald's ActivityPub actor cannot archive or lock a thread".to_owned(),
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
            Self::Slack(slack) => Posts::Slack(SlackMarkers::new(slack, http)),
            Self::ActivityPub(_) => Posts::Unread(UNREAD),
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

/// Why herald's own actor reads nothing back: it serves its Notes rather
/// than posting them somewhere it could read.
const UNREAD: &str = "herald's ActivityPub actor reads no posts back";

/// One route's destination's own posts, read back for their markers.
#[derive(Debug)]
pub enum Posts<'a, H> {
    /// A Discord channel's.
    Discord(DiscordMarkers<'a, H>),
    /// A Mastodon account's.
    Mastodon(MastodonMarkers<'a, H>),
    /// A Matrix room's.
    Matrix(MatrixMarkers<'a, H>),
    /// A Slack channel's.
    Slack(SlackMarkers<'a, H>),
    /// None to read, and why.
    Unread(&'static str),
}

impl<H: Http> Markers for Posts<'_, H> {
    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.entry(key, marker).await,
            Self::Mastodon(posts) => posts.entry(key, marker).await,
            Self::Matrix(posts) => posts.entry(key, marker).await,
            Self::Slack(posts) => posts.entry(key, marker).await,
            Self::Unread(why) => Err(MarkersError::Unreadable {
                reason: (*why).to_owned(),
            }),
        }
    }

    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        match self {
            Self::Discord(posts) => posts.threads(route).await,
            Self::Mastodon(posts) => posts.threads(route).await,
            Self::Matrix(posts) => posts.threads(route).await,
            Self::Slack(posts) => posts.threads(route).await,
            Self::Unread(why) => Err(MarkersError::Unreadable {
                reason: (*why).to_owned(),
            }),
        }
    }
}
