use herald_core::{Entry, Facts, Key, Marker, Marks, Step};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use serde_json::Value;

use crate::mastodon::Mastodon;
use crate::read::text;
use crate::request::get;

/// The most statuses Mastodon lists in one page.
const PAGE: u8 = 40;
/// What opens a link's target in a status's HTML.
const HREF: &str = "href=\"";

/// A Mastodon account's statuses read back for the markers herald's own
/// statuses carry (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct MastodonMarkers<'a, H> {
    mastodon: &'a Mastodon,
    http: &'a H,
}

impl<'a, H: Http> MastodonMarkers<'a, H> {
    /// The markers in the statuses of the account `mastodon` posts as, read
    /// through `http`.
    #[must_use]
    pub fn new(mastodon: &'a Mastodon, http: &'a H) -> Self {
        Self { mastodon, http }
    }

    async fn read(&self, path: &str) -> Result<Value, MarkersError> {
        let response = self
            .http
            .send(get(self.mastodon, path))
            .await
            .map_err(|error| MarkersError::Unreadable {
                reason: error.to_string(),
            })?;
        if !response.succeeded() {
            return Err(MarkersError::Unreadable {
                reason: format!("Mastodon answered {}", response.status),
            });
        }
        Ok(serde_json::from_slice(&response.body).unwrap_or(Value::Null))
    }

    async fn statuses(&self) -> Result<Vec<Value>, MarkersError> {
        let account = self.read("/api/v1/accounts/verify_credentials").await?;
        let herald = text(&account, "id").ok_or_else(|| MarkersError::Unreadable {
            reason: "Mastodon did not say which account the token is".to_owned(),
        })?;
        let page = self
            .read(&format!(
                "/api/v1/accounts/{herald}/statuses?limit={PAGE}&exclude_reblogs=true"
            ))
            .await?;
        Ok(page
            .as_array()
            .into_iter()
            .flatten()
            .filter(|status| {
                status
                    .get("account")
                    .and_then(|account| text(account, "id"))
                    .is_some_and(|author| author == herald)
            })
            .cloned()
            .collect())
    }
}

/// The announcement markers the links of `status` carry.
fn carried(status: &Value) -> Vec<String> {
    let content = status
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    content
        .split(HREF)
        .skip(1)
        .filter_map(|rest| rest.split_once('"'))
        .filter_map(|(link, _)| Marks::carried(link))
        .map(|(_, announcement)| announcement.to_owned())
        .collect()
}

fn reply_to(status: &Value) -> Option<String> {
    text(status, "in_reply_to_id")
}

fn landed(status: &Value) -> Facts {
    Facts {
        message: text(status, "id"),
        thread: None,
        permalink: text(status, "url"),
    }
}

impl<H: Http> Markers for MastodonMarkers<'_, H> {
    fn threads(
        &self,
        _: &Marker,
    ) -> impl Future<Output = Result<Vec<MarkedThread>, MarkersError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        let statuses = self.statuses().await?;
        let Some(post) = statuses.iter().find(|status| {
            carried(status)
                .iter()
                .any(|carried| carried == marker.as_str())
        }) else {
            return Ok(None);
        };
        let mut entry = Entry::new(*key).landed(Step::Post, landed(post));
        let mut before = text(post, "id");
        for index in 1..=u32::try_from(statuses.len()).unwrap_or(u32::MAX) {
            let Some(reply) = before.as_deref().and_then(|id| {
                statuses
                    .iter()
                    .find(|status| reply_to(status).as_deref() == Some(id))
            }) else {
                break;
            };
            entry = entry.landed(Step::Chunk(index), landed(reply));
            before = text(reply, "id");
        }
        Ok(Some(entry))
    }
}
