//! An `Http` answering each request with the next scripted answer.

use std::collections::VecDeque;
use std::sync::Mutex;

use herald_ports::{Http, HttpError, Request, Response};

/// The headers of each answer, in order.
type Heads = std::sync::Arc<Mutex<VecDeque<Vec<(String, String)>>>>;

#[derive(Debug, Clone, Default)]
pub(crate) struct Script {
    pub(crate) answers: std::sync::Arc<Mutex<VecDeque<(String, u16, String)>>>,
    seen: std::sync::Arc<Mutex<Vec<String>>>,
    bodies: std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
    headers: Vec<(String, String)>,
    heads: Heads,
}

impl Script {
    pub(crate) fn answering(answers: &[(&str, u16, &str)]) -> Self {
        let script = Self::default();
        script.answers.lock().unwrap().extend(
            answers
                .iter()
                .map(|(line, status, body)| ((*line).to_owned(), *status, (*body).to_owned())),
        );
        script
    }

    pub(crate) fn headed(mut self, headers: &[(&str, &str)]) -> Self {
        self.headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self
    }

    /// The script answering each request with the next of `answers`, each
    /// with its own headers.
    pub(crate) fn answered(answers: Vec<(String, Response)>) -> Self {
        let script = Self::default();
        for (line, response) in answers {
            script.answers.lock().unwrap().push_back((
                line,
                response.status,
                String::from_utf8(response.body).unwrap(),
            ));
            script.heads.lock().unwrap().push_back(response.headers);
        }
        script
    }

    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn bodies(&self) -> Vec<Vec<u8>> {
        self.bodies.lock().unwrap().clone()
    }
}

impl Http for Script {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen.lock().unwrap().push(line.clone());
        self.bodies.lock().unwrap().push(request.body.clone());
        let Some((expected, status, body)) = self.answers.lock().unwrap().pop_front() else {
            return Err(HttpError::Unreachable {
                reason: format!("nothing scripted for {line}"),
            });
        };
        assert!(line.ends_with(&expected), "{line} is not {expected}");
        if body.len() > request.most {
            return Err(HttpError::Unreachable {
                reason: format!("the response is larger than {} bytes", request.most),
            });
        }
        let headers = self
            .heads
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| self.headers.clone());
        Ok(Response {
            status,
            headers,
            body: body.into_bytes(),
        })
    }
}
