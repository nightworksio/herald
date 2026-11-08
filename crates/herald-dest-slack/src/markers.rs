use herald_core::{Entry, Facts, Key, Marker, Sent, Step};
use herald_ports::{Http, MarkedThread, Markers, MarkersError, unreserved};
use serde_json::Value;

use crate::read::{json, text, trouble};
use crate::request::{MARKER, get};
use crate::slack::{Credential, Identity, Slack, carried, identity};
use crate::web_api::{CONVERSATIONS_HISTORY, CONVERSATIONS_REPLIES, WebMethod};

/// The most messages read back from a channel, or from a thread.
const PAGE: u8 = 100;

/// A Slack channel read back for the markers herald's own messages carry in
/// their metadata (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct SlackMarkers<'a, H> {
    slack: &'a Slack,
    http: &'a H,
}

fn unreadable(reason: impl Into<String>) -> MarkersError {
    MarkersError::Unreadable {
        reason: reason.into(),
    }
}

fn said(sent: Sent) -> MarkersError {
    unreadable(match sent {
        Sent::Refused(reason) => reason,
        Sent::RetryAfter(wait) => format!("Slack asked to wait {wait:#} first"),
        Sent::Landed(_) => "Slack answered nothing to read".to_owned(),
    })
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
    pub fn new(slack: &'a Slack, http: &'a H) -> Self {
        Self { slack, http }
    }

    async fn read(&self, method: WebMethod, asked: &str) -> Result<Value, MarkersError> {
        let response = self
            .http
            .send(get(self.slack, method, asked))
            .await
            .map_err(|error| unreadable(error.to_string()))?;
        let body = json(&response);
        trouble(&response, &body).map_or(Ok(body), |sent| Err(said(sent)))
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
        for _ in 0..self.slack.read_back.pages() {
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

    fn channel(&self) -> Result<&str, MarkersError> {
        if let Credential::Webhook(_) = self.slack.credential {
            return Err(unreadable("a webhook cannot read a channel's messages"));
        }
        self.slack
            .route
            .channel
            .as_deref()
            .ok_or_else(|| unreadable("a Slack bot posts to a channel, and the route names none"))
    }

    async fn identity(&self) -> Result<Identity, MarkersError> {
        identity(self.slack, self.http)
            .await
            .map_err(|error| unreadable(error.to_string()))?
            .map_err(said)
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

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;

    #[test]
    fn what_slack_answered_instead_of_a_page_reads_as_a_reason() {
        for (sent, reason) in [
            (Sent::Refused("no".to_owned()), "no"),
            (
                Sent::RetryAfter(SignedDuration::from_secs(3)),
                "Slack asked to wait 3s first",
            ),
            (
                Sent::Landed(Facts::default()),
                "Slack answered nothing to read",
            ),
        ] {
            assert_eq!(
                said(sent),
                MarkersError::Unreadable {
                    reason: reason.to_owned()
                }
            );
        }
    }
}
