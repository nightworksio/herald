use herald_core::{Entry, Facts, Key, Marker, Step};
use herald_ports::{Http, MarkedThread, Markers, MarkersError, unreserved};
use serde_json::Value;

use crate::matrix::Matrix;
use crate::read::{UNNAMED, json, text};
use crate::request::{MARKER, MESSAGE, get, pinned, pins};

/// The most events read back from a room's timeline.
const PAGE: u8 = 100;

/// A Matrix room read back for the markers herald's own messages carry
/// (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct MatrixMarkers<'a, H> {
    matrix: &'a Matrix,
    http: H,
}

fn unreadable(reason: impl Into<String>) -> MarkersError {
    MarkersError::Unreadable {
        reason: reason.into(),
    }
}

impl<'a, H: Http> MatrixMarkers<'a, H> {
    /// The markers in the room `matrix` posts to, read through `http`.
    #[must_use]
    pub fn new(matrix: &'a Matrix, http: H) -> Self {
        Self { matrix, http }
    }

    async fn read(&self, path: &str) -> Result<Value, MarkersError> {
        let response = self
            .http
            .send(get(self.matrix, path))
            .await
            .map_err(|error| unreadable(error.to_string()))?;
        if !response.succeeded() {
            return Err(unreadable(format!(
                "the homeserver answered {}",
                response.status
            )));
        }
        Ok(json(&response))
    }

    /// The room's recent messages that herald's own account sent, newest
    /// first, read back page by page until a page holds one `sought`, or
    /// none is left.
    async fn own(&self, sought: impl Fn(&Value) -> bool) -> Result<Vec<Value>, MarkersError> {
        let account = self.read("/account/whoami").await?;
        let herald = text(&account, "user_id")
            .ok_or_else(|| unreadable(UNNAMED))?
            .to_owned();
        let room = unreserved(&self.matrix.route.room);
        let mut own = Vec::new();
        let mut from: Option<String> = None;
        for _ in 0..self.matrix.read_back.pages() {
            let path = from.as_deref().map_or_else(
                || format!("/rooms/{room}/messages?dir=b&limit={PAGE}"),
                |from| {
                    format!(
                        "/rooms/{room}/messages?dir=b&limit={PAGE}&from={}",
                        unreserved(from)
                    )
                },
            );
            let page = self.read(&path).await?;
            let chunk = page
                .get("chunk")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let mine: Vec<Value> = chunk
                .iter()
                .filter(|event| text(event, "type") == Some(MESSAGE))
                .filter(|event| text(event, "sender") == Some(herald.as_str()))
                .cloned()
                .collect();
            let found = mine.iter().any(&sought);
            own.extend(mine);
            from = text(&page, "end").map(ToOwned::to_owned);
            if found || from.is_none() {
                break;
            }
        }
        Ok(own)
    }
}

fn content(event: &Value) -> &Value {
    event.get("content").unwrap_or(&Value::Null)
}

fn carries(event: &Value, marker: &Marker) -> bool {
    text(content(event), MARKER) == Some(marker.as_str())
}

fn threaded_under(event: &Value, root: &str) -> bool {
    let relation = content(event).get("m.relates_to").unwrap_or(&Value::Null);
    text(relation, "rel_type") == Some("m.thread") && text(relation, "event_id") == Some(root)
}

fn landed(event: &Value) -> Facts {
    Facts {
        message: text(event, "event_id").map(ToOwned::to_owned),
        ..Facts::default()
    }
}

impl<H: Http> Markers for MatrixMarkers<'_, H> {
    fn threads(
        &self,
        _: &Marker,
    ) -> impl Future<Output = Result<Vec<MarkedThread>, MarkersError>> + Send {
        std::future::ready(Ok(Vec::new()))
    }

    async fn entry(&self, key: &Key, marker: &Marker) -> Result<Option<Entry>, MarkersError> {
        let events = self.own(|event| carries(event, marker)).await?;
        let Some(post) = events.iter().find(|event| carries(event, marker)) else {
            return Ok(None);
        };
        let Some(root) = text(post, "event_id") else {
            return Ok(None);
        };
        let mut entry = Entry::new(*key).landed(Step::Post, landed(post));
        let mut replies: Vec<&Value> = events
            .iter()
            .filter(|event| threaded_under(event, root))
            .collect();
        replies.reverse();
        for (index, reply) in (1..).zip(replies) {
            entry = entry.landed(Step::Chunk(index), landed(reply));
        }
        let response = self
            .http
            .send(pinned(self.matrix))
            .await
            .map_err(|error| unreadable(error.to_string()))?;
        let held = pins(&response, self.matrix.waits).map_err(|_| {
            unreadable(format!(
                "the homeserver answered {} for the room's pins",
                response.status
            ))
        })?;
        if held.iter().any(|pinned| pinned == root) {
            entry = entry
                .landed(Step::Pin, Facts::default())
                .landed(Step::Unpin, Facts::default());
        }
        Ok(Some(entry))
    }
}
