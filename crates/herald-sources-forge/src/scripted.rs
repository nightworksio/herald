//! A forge answering each request with the next scripted answer.

use std::collections::VecDeque;
use std::sync::Mutex;

use herald_ports::{Http, HttpError, Request, Response};
use serde_json::Value;

/// A forge answering each request with the next scripted response, and
/// keeping each request.
pub(crate) struct Scripted {
    answers: Mutex<VecDeque<Response>>,
    requests: Mutex<Vec<Request>>,
}

impl Scripted {
    pub(crate) fn new(answers: Vec<Response>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Each request's URL and `If-None-Match`, in order.
    pub(crate) fn asked(&self) -> Vec<(String, Option<String>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| {
                let etag = request
                    .headers
                    .iter()
                    .find(|(name, _)| name == "If-None-Match")
                    .map(|(_, value)| value.redacted());
                (request.url.redacted(), etag)
            })
            .collect()
    }

    /// Each request, in order.
    pub(crate) fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// Each request's body, read as JSON, in order.
    pub(crate) fn bodies(&self) -> Vec<Value> {
        self.requests()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap())
            .collect()
    }
}

impl Http for Scripted {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        self.requests.lock().unwrap().push(request);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(HttpError::TimedOut)
    }
}
