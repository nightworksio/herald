//! An `Http` answering each request with the next scripted answer.

use std::collections::VecDeque;
use std::sync::Mutex;

use herald_ports::{Http, HttpError, Request, Response};
use serde_json::Value;

use crate::lock;

/// An `Http` answering each request with the next scripted response, and
/// keeping each request; a request past the last answer times out.
#[derive(Debug)]
pub struct ScriptedHttp {
    answers: Mutex<VecDeque<Response>>,
    requests: Mutex<Vec<Request>>,
}

impl ScriptedHttp {
    /// An `Http` answering with `answers`, in order.
    #[must_use]
    pub fn new(answers: Vec<Response>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Each request's URL and `If-None-Match`, in order.
    #[must_use]
    pub fn asked(&self) -> Vec<(String, Option<String>)> {
        lock(&self.requests)
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
    #[must_use]
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.requests).clone()
    }

    /// Each request's body, read as JSON, in order; one that is not JSON
    /// reads as `null`.
    #[must_use]
    pub fn bodies(&self) -> Vec<Value> {
        self.requests()
            .iter()
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }
}

impl Http for ScriptedHttp {
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        lock(&self.requests).push(request);
        lock(&self.answers).pop_front().ok_or(HttpError::TimedOut)
    }
}

#[cfg(test)]
mod tests {
    use herald_ports::{Method, SecretText};

    use super::*;

    #[test]
    fn a_scripted_http_answers_in_order_keeps_each_request_and_then_times_out() {
        let http = ScriptedHttp::new(vec![Response {
            status: 200,
            headers: Vec::new(),
            body: b"{}".to_vec(),
        }]);
        let first = Request::new(Method::Post, SecretText::plain("https://a.example/x"))
            .conditional(Some("\"e\""), None);
        let first = Request {
            body: br#"{"a":1}"#.to_vec(),
            ..first
        };
        let second = Request::new(Method::Get, SecretText::plain("https://a.example/y"));
        assert_eq!(pollster::block_on(http.send(first)).unwrap().status, 200);
        assert_eq!(
            pollster::block_on(http.send(second)),
            Err(HttpError::TimedOut)
        );
        assert_eq!(
            http.asked(),
            [
                ("https://a.example/x".to_owned(), Some("\"e\"".to_owned())),
                ("https://a.example/y".to_owned(), None)
            ]
        );
        assert_eq!(http.requests().len(), 2);
        assert_eq!(http.requests()[1].url.expose(), "https://a.example/y");
        assert_eq!(http.bodies(), [serde_json::json!({"a": 1}), Value::Null]);
    }
}
