use herald_core::{Entry, Facts, Key, Marker, Step, unreserved};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use serde_json::Value;

use crate::read::{failure, json, text};
use crate::request::{MARKER, channel, get, webhook_cannot};
use crate::slack::{Identity, Slack, carried, identity};
use crate::web_api::{CONVERSATIONS_HISTORY, CONVERSATIONS_REPLIES, WebMethod};

/// The most messages read back from a channel, or from a thread.
const PAGE: u8 = 100;

/// A Slack channel read back for the markers herald's own messages carry in
/// their metadata (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct SlackMarkers<'a, H> {
    slack: &'a Slack,
    http: H,
}

fn unreadable(reason: impl Into<String>) -> MarkersError {
    MarkersError::Unreadable {
        reason: reason.into(),
    }
}

fn messages(page: &Value) -> impl Iterator<Item = &Value> {
    page.get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn landed(message: &Value) -> Facts {
    Facts {
        message: text(message, "ts").map(ToOwned::to_owned),
        ..Facts::default()
    }
}

impl<'a, H: Http> SlackMarkers<'a, H> {
    /// The markers in the channel `slack` posts to, read through `http`.
    #[must_use]
    pub fn new(slack: &'a Slack, http: H) -> Self {
        Self { slack, http }
    }

    async fn read(&self, method: WebMethod, asked: &str) -> Result<Value, MarkersError> {
        let response = self.http.send(get(self.slack, method, asked)).await?;
        let body = json(&response);
        failure(&response, &body, self.slack.waits)
            .map_or(Ok(body), |sent| Err(MarkersError::unlanded(sent)))
    }

    /// What `method` lists for `asked`, read page by page until a page
    /// holds a message `sought`, or none is left.
    async fn paged(
        &self,
        method: WebMethod,
        asked: &str,
        sought: impl Fn(&Value) -> bool,
    ) -> Result<Vec<Value>, MarkersError> {
        let mut listed = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..self.slack.read_back.get() {
            let paging = cursor.as_deref().map_or_else(
                || asked.to_owned(),
                |cursor| format!("{asked}&cursor={}", unreserved(cursor)),
            );
            let page = self.read(method, &paging).await?;
            let found = messages(&page).any(&sought);
            listed.extend(messages(&page).cloned());
            cursor = page
                .get("response_metadata")
                .and_then(|metadata| text(metadata, "next_cursor"))
                .filter(|next| !next.is_empty())
                .map(ToOwned::to_owned);
            if found || cursor.is_none() {
                break;
            }
        }
        Ok(listed)
    }

    /// The channel read back, where a bot reads one.
    fn channel(&self) -> Result<&str, MarkersError> {
        let declined = match self.slack.credential.bot() {
            Some(_) => channel(self.slack),
            None => Err(webhook_cannot("read a channel's messages")),
        };
        declined.map_err(|declined| unreadable(declined.to_string()))
    }

    async fn identity(&self) -> Result<Identity, MarkersError> {
        identity(self.slack, &self.http)
            .await?
            .map_err(MarkersError::unlanded)
    }
}

impl<H: Http> Markers for SlackMarkers<'_, H> {
    fn threads(
        &self,
        _: &Marker,
    ) -> impl Future<Output = Result<Vec<MarkedThread>, MarkersError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        let channel = self.channel()?;
        let identity = self.identity().await?;
        let posted = |message: &Value| {
            identity.posted(message) && carried(message, MARKER) == Some(marker.as_str())
        };
        let history = self
            .paged(
                CONVERSATIONS_HISTORY,
                &format!(
                    "channel={}&limit={PAGE}&include_all_metadata=true",
                    unreserved(channel)
                ),
                posted,
            )
            .await?;
        let Some(post) = history.iter().find(|message| posted(message)) else {
            return Ok(None);
        };
        let Some(root) = text(post, "ts") else {
            return Ok(None);
        };
        let mut entry = Entry::new(*key).landed(Step::Post, landed(post));
        let thread = self
            .paged(
                CONVERSATIONS_REPLIES,
                &format!(
                    "channel={}&ts={}&limit={PAGE}",
                    unreserved(channel),
                    unreserved(root)
                ),
                |_| false,
            )
            .await?;
        let replies = thread
            .iter()
            .filter(|reply| text(reply, "ts") != Some(root))
            .filter(|reply| text(reply, "thread_ts") == Some(root))
            .filter(|reply| identity.posted(reply));
        for (index, reply) in (1..).zip(replies) {
            entry = entry.landed(Step::Chunk(index), landed(reply));
        }
        let pinned = post
            .get("pinned_to")
            .and_then(Value::as_array)
            .is_some_and(|pins| pins.iter().any(|pinned| pinned.as_str() == Some(channel)));
        if pinned {
            entry = entry
                .landed(Step::Pin, Facts::default())
                .landed(Step::Unpin, Facts::default());
        }
        Ok(Some(entry))
    }
}
