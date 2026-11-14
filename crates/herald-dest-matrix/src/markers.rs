use herald_core::{Entry, Facts, Key, Marker, Step, unreserved};
use herald_ports::{Http, MarkedThread, Markers, MarkersError};
use serde_json::Value;

use crate::matrix::Matrix;
use crate::read::{failure, json, text};
use crate::request::{MARKER, MESSAGE, THREAD, get, pinned, pins};

/// The most events read back from a room's timeline.
const PAGE: u8 = 100;

/// A Matrix room read back for the markers herald's own messages carry
/// (ADR-0005, ADR-0011).
#[derive(Debug)]
pub struct MatrixMarkers<'a, H> {
    matrix: &'a Matrix,
    http: H,
}

impl<'a, H: Http> MatrixMarkers<'a, H> {
    /// The markers in the room `matrix` posts to, read through `http`.
    #[must_use]
    pub fn new(matrix: &'a Matrix, http: H) -> Self {
        Self { matrix, http }
    }

    async fn read(&self, path: &str) -> Result<Value, MarkersError> {
        let response = self.http.send(get(self.matrix, path)).await?;
        match failure(&response, self.matrix.waits) {
            Some(sent) => Err(MarkersError::unlanded(sent)),
            None => Ok(json(&response)),
        }
    }

    /// The room's recent messages that herald's own account sent, newest
    /// first, read back page by page until a page holds one `sought`, or
    /// none is left.
    async fn own(&self, sought: impl Fn(&Value) -> bool) -> Result<Vec<Value>, MarkersError> {
        let herald = self
            .matrix
            .account(&self.http)
            .await?
            .map_err(MarkersError::unlanded)?;
        let room = unreserved(&self.matrix.route.room);
        let mut own = Vec::new();
        let mut from: Option<String> = None;
        for _ in 0..self.matrix.read_back.get() {
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
    text(relation, "rel_type") == Some(THREAD) && text(relation, "event_id") == Some(root)
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
        let response = self.http.send(pinned(self.matrix)).await?;
        let held = pins(&response, self.matrix.waits).map_err(MarkersError::unlanded)?;
        if held.iter().any(|pinned| pinned == root) {
            entry = entry
                .landed(Step::Pin, Facts::default())
                .landed(Step::Unpin, Facts::default());
        }
        Ok(Some(entry))
    }
}
