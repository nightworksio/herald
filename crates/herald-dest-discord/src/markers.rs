use herald_core::{Entry, Facts, Key, Marker, Marks, Step, unreserved};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use jiff::Timestamp;
use serde_json::Value;

use crate::discord::Discord;
use crate::read::{failure, json, owned};
use crate::request::{channel, get, webhook_cannot};

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
    http: H,
}

impl<'a, H: Http> DiscordMarkers<'a, H> {
    /// The markers in the channel `discord` posts to, read through `http`.
    #[must_use]
    pub fn new(discord: &'a Discord, http: H) -> Self {
        Self { discord, http }
    }

    async fn read(&self, path: &str) -> Result<Value, MarkersError> {
        let response = self.http.send(get(self.discord, path)).await?;
        match failure(&response, self.discord.waits) {
            Some(sent) => Err(MarkersError::unlanded(sent)),
            None => Ok(json(&response)),
        }
    }

    async fn herald(&self) -> Result<String, MarkersError> {
        if let Some(bot) = self.discord.bot.get() {
            return Ok(bot.clone());
        }
        let user = self.read("/users/@me").await?;
        let bot = owned(&user, "id").ok_or_else(|| MarkersError::Unreadable {
            reason: crate::unpin::UNNAMED.to_owned(),
        })?;
        Ok(self.discord.bot.learn(bot).clone())
    }

    /// The channel read back, where a bot reads one.
    fn channel(&self) -> Result<&str, MarkersError> {
        let declined = match self.discord.credential.bot() {
            Some(_) => channel(self.discord),
            None => Err(webhook_cannot("read a channel's messages")),
        };
        declined.map_err(|declined| MarkersError::Unreadable {
            reason: declined.to_string(),
        })
    }

    /// The messages `author` posted in `channel`, newest first, read back
    /// page by page until a page holds one `sought`, or none is left.
    async fn messages(
        &self,
        channel: &str,
        author: &str,
        sought: impl Fn(&Value) -> bool,
    ) -> Result<Vec<Value>, MarkersError> {
        let mut own = Vec::new();
        let mut before: Option<String> = None;
        for _ in 0..self.discord.read_back.pages() {
            let path = before.as_deref().map_or_else(
                || format!("/channels/{channel}/messages?limit={PAGE}"),
                |id| {
                    format!(
                        "/channels/{channel}/messages?limit={PAGE}&before={}",
                        unreserved(id)
                    )
                },
            );
            let page = self.read(&path).await?;
            let listed = page.as_array().map(Vec::as_slice).unwrap_or_default();
            let mine: Vec<Value> = listed
                .iter()
                .filter(|message| self::author(message) == Some(author))
                .cloned()
                .collect();
            let done = listed.len() < usize::from(PAGE) || mine.iter().any(&sought);
            own.extend(mine);
            before = listed.last().and_then(|message| owned(message, "id"));
            if done || before.is_none() {
                break;
            }
        }
        Ok(own)
    }
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
        .filter_map(Marks::carried)
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
        thread: owned(thread, "id")?,
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
        let channel = self.channel()?;
        let herald = self.herald().await?;
        Ok(self
            .messages(channel, &herald, |_| false)
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
        let channel = self.channel()?;
        let herald = self.herald().await?;
        let messages = self
            .messages(channel, &herald, |message| carries(message, marker))
            .await?;
        let Some(post) = messages.iter().find(|message| carries(message, marker)) else {
            return Ok(None);
        };
        let message = owned(post, "id");
        let thread = post.get("thread").and_then(|thread| owned(thread, "id"));
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
            .messages(&thread, &herald, |_| false)
            .await?
            .iter()
            .filter(|chunk| chunk.get("type").and_then(Value::as_u64) == Some(DEFAULT_TYPE))
            .filter(|chunk| owned(chunk, "id") != message)
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
