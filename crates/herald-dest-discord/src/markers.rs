use herald_core::{Entry, Facts, Key, Marker, Step};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use jiff::Timestamp;
use serde_json::Value;

use crate::discord::{Credential, Discord};
use crate::request::get;

/// The most messages Discord lists in one page.
const PAGE: u8 = 100;
/// The flag of a message that has been crossposted.
const CROSSPOSTED: u64 = 1;
/// The type of an ordinary message, as a chunk is.
const DEFAULT_TYPE: u64 = 0;

/// A Discord channel read back for the markers herald's own posts carry
/// (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct DiscordMarkers<'a, H> {
    discord: &'a Discord,
    http: &'a H,
}

impl<'a, H: Http> DiscordMarkers<'a, H> {
    /// The markers in the channel `discord` posts to, read through `http`.
    #[must_use]
    pub fn new(discord: &'a Discord, http: &'a H) -> Self {
        Self { discord, http }
    }

    async fn read(&self, path: &str) -> Result<Value, MarkersError> {
        if let Credential::Webhook(_) = &self.discord.credential {
            return Err(MarkersError::Unreadable {
                reason: "a webhook cannot read a channel's messages".to_owned(),
            });
        }
        let request = get(self.discord, path);
        let response = self
            .http
            .send(request)
            .await
            .map_err(|error| MarkersError::Unreadable {
                reason: error.to_string(),
            })?;
        if !response.succeeded() {
            return Err(MarkersError::Unreadable {
                reason: format!("Discord answered {}", response.status),
            });
        }
        Ok(serde_json::from_slice(&response.body).unwrap_or(Value::Null))
    }

    async fn herald(&self) -> Result<String, MarkersError> {
        let user = self.read("/users/@me").await?;
        text(&user, "id").ok_or_else(|| MarkersError::Unreadable {
            reason: "Discord did not say which user the bot is".to_owned(),
        })
    }

    async fn messages(&self, channel: &str, author: &str) -> Result<Vec<Value>, MarkersError> {
        let page = self
            .read(&format!("/channels/{channel}/messages?limit={PAGE}"))
            .await?;
        Ok(page
            .as_array()
            .into_iter()
            .flatten()
            .filter(|message| self::author(message) == Some(author))
            .cloned()
            .collect())
    }
}

fn text(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// The route and announcement markers on `message`'s embed links, as
/// written: `#herald-<route>.<announcement>`.
pub(crate) fn marks(message: &Value) -> Vec<(String, String)> {
    message
        .get("embeds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|embed| embed.get("url").and_then(Value::as_str))
        .filter_map(|url| url.rsplit_once("#herald-"))
        .filter_map(|(_, fragment)| fragment.split_once('.'))
        .map(|(route, announcement)| (route.to_owned(), announcement.to_owned()))
        .collect()
}

pub(crate) fn author(message: &Value) -> Option<&str> {
    message
        .get("author")
        .and_then(|user| user.get("id"))
        .and_then(Value::as_str)
}

fn timestamp(value: &Value, field: &str) -> Option<Timestamp> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(|text| text.parse().ok())
}

fn flag(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool) == Some(true)
}

/// The thread opened from `post`, as the post describes it: opened when
/// Discord says, or else when the post was made.
fn marked_thread(post: &Value) -> Option<MarkedThread> {
    let thread = post.get("thread")?;
    let metadata = thread.get("thread_metadata").unwrap_or(&Value::Null);
    Some(MarkedThread {
        thread: text(thread, "id")?,
        opened: timestamp(metadata, "create_timestamp").or_else(|| timestamp(post, "timestamp"))?,
        archived: flag(metadata, "archived"),
        locked: flag(metadata, "locked"),
    })
}

fn carries(message: &Value, marker: &Marker) -> bool {
    marks(message)
        .iter()
        .any(|(_, announcement)| announcement == marker.as_str())
}

impl<H: Http> Markers for DiscordMarkers<'_, H> {
    async fn threads(&self, route: &Marker) -> Result<Vec<MarkedThread>, MarkersError> {
        let channel = self.discord.route.channel.as_deref().unwrap_or_default();
        let herald = self.herald().await?;
        Ok(self
            .messages(channel, &herald)
            .await?
            .iter()
            .filter(|post| {
                marks(post)
                    .iter()
                    .any(|(carried, _)| carried == route.as_str())
            })
            .filter_map(marked_thread)
            .collect())
    }

    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        let channel = self.discord.route.channel.as_deref().unwrap_or_default();
        let herald = self.herald().await?;
        let messages = self.messages(channel, &herald).await?;
        let Some(post) = messages.iter().find(|message| carries(message, marker)) else {
            return Ok(None);
        };
        let message = text(post, "id");
        let thread = post.get("thread").and_then(|thread| text(thread, "id"));
        let mut entry = Entry::new(*key).landed(
            Step::Post,
            Facts {
                message: message.clone(),
                ..Facts::default()
            },
        );
        if post
            .get("flags")
            .and_then(Value::as_u64)
            .is_some_and(|flags| flags & CROSSPOSTED != 0)
        {
            entry = entry.landed(
                Step::Crosspost,
                Facts {
                    message: message.clone(),
                    ..Facts::default()
                },
            );
        }
        if flag(post, "pinned") {
            entry = entry
                .landed(Step::Pin, Facts::default())
                .landed(Step::Unpin, Facts::default());
        }
        let Some(thread) = thread else {
            return Ok(Some(entry));
        };
        entry = entry.landed(
            Step::OpenThread,
            Facts {
                thread: Some(thread.clone()),
                ..Facts::default()
            },
        );
        let chunks = self
            .messages(&thread, &herald)
            .await?
            .iter()
            .filter(|chunk| chunk.get("type").and_then(Value::as_u64) == Some(DEFAULT_TYPE))
            .filter(|chunk| text(chunk, "id") != message)
            .count();
        for index in 1..=u32::try_from(chunks).unwrap_or(u32::MAX) {
            entry = entry.landed(
                Step::Chunk(index),
                Facts {
                    thread: Some(thread.clone()),
                    ..Facts::default()
                },
            );
        }
        Ok(Some(entry))
    }
}
