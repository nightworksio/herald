use herald_core::{Entry, Facts, Key, Marker, Sent, Step};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use serde_json::Value;

use crate::read::{json, text, trouble};
use crate::request::{MARKER, get, query};
use crate::slack::{Credential, Identity, Slack, carried, identity};

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

    async fn read(&self, method: &str, asked: &str) -> Result<Value, MarkersError> {
        let response = self
            .http
            .send(get(self.slack, method, asked))
            .await
            .map_err(|error| unreadable(error.to_string()))?;
        let body = json(&response);
        trouble(&response, &body).map_or(Ok(body), |sent| Err(said(sent)))
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
        let history = self
            .read(
                "conversations.history",
                &format!(
                    "channel={}&limit={PAGE}&include_all_metadata=true",
                    query(channel)
                ),
            )
            .await?;
        let Some(post) = messages(&history).find(|message| {
            identity.posted(message) && carried(message, MARKER) == Some(marker.as_str())
        }) else {
            return Ok(None);
        };
        let Some(root) = text(post, "ts") else {
            return Ok(None);
        };
        let mut entry = Entry::new(*key).landed(Step::Post, landed(post));
        let thread = self
            .read(
                "conversations.replies",
                &format!("channel={}&ts={}&limit={PAGE}", query(channel), query(root)),
            )
            .await?;
        let replies = messages(&thread)
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
