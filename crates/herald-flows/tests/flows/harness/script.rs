//! An `Http` answering each request with the next scripted answer.

use std::collections::VecDeque;
use std::sync::Mutex;

use herald_ports::{Http, HttpError, Request, Response};

/// The status a script answers with where no connection is made for the
/// request, its body the transport's reason.
pub(crate) const UNCONNECTED: u16 = 0;

/// The headers of each answer, in order.
type Heads = std::sync::Arc<Mutex<VecDeque<Vec<(String, String)>>>>;

#[derive(Debug, Clone, Default)]
pub(crate) struct Script {
    pub(crate) answers: std::sync::Arc<Mutex<VecDeque<(String, u16, String)>>>,
    seen: std::sync::Arc<Mutex<Vec<String>>>,
    bodies: std::sync::Arc<Mutex<Vec<Vec<u8>>>>,
    asked: Heads,
    most: std::sync::Arc<Mutex<Vec<usize>>>,
    headers: Vec<(String, String)>,
    heads: Heads,
    made: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    keyed: std::sync::Arc<std::collections::BTreeMap<String, Response>>,
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

    /// The script answering each request by its URL, whatever the order,
    /// with `keyed`'s answer to it, else 404.
    pub(crate) fn keyed(keyed: std::collections::BTreeMap<String, Response>) -> Self {
        Self {
            keyed: std::sync::Arc::new(keyed),
            ..Self::default()
        }
    }

    pub(crate) fn then(self, answers: &[(&str, u16, &str)]) -> Self {
        self.answers.lock().unwrap().extend(
            answers
                .iter()
                .map(|(line, status, body)| ((*line).to_owned(), *status, (*body).to_owned())),
        );
        self
    }

    /// How many requests it was sent.
    pub(crate) fn made(&self) -> &std::sync::atomic::AtomicUsize {
        &self.made
    }

    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    pub(crate) fn bodies(&self) -> Vec<Vec<u8>> {
        self.bodies.lock().unwrap().clone()
    }

    /// The most bytes of its answer each request reads, in order.
    pub(crate) fn most(&self) -> Vec<usize> {
        self.most.lock().unwrap().clone()
    }

    /// The headers of each request, in order.
    pub(crate) fn asked(&self) -> Vec<Vec<(String, String)>> {
        self.asked.lock().unwrap().iter().cloned().collect()
    }
}

impl Http for Script {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        self.made.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let line = format!("{} {}", request.method.as_str(), request.url.redacted());
        self.seen.lock().unwrap().push(line.clone());
        self.bodies.lock().unwrap().push(request.body.clone());
        self.most.lock().unwrap().push(request.most);
        self.asked.lock().unwrap().push_back(
            request
                .headers
                .iter()
                .map(|(name, value)| (name.clone(), value.expose()))
                .collect(),
        );
        if let Some(keyed) = self.keyed.get(&request.url.redacted()) {
            return Ok(keyed.clone());
        }
        if !self.keyed.is_empty() && self.answers.lock().unwrap().is_empty() {
            return Ok(self
                .keyed
                .get(&request.url.redacted())
                .cloned()
                .unwrap_or(Response {
                    status: 404,
                    headers: Vec::new(),
                    body: b"{}".to_vec(),
                }));
        }
        let Some((expected, status, body)) = self.answers.lock().unwrap().pop_front() else {
            return Err(HttpError::Unreachable {
                reason: format!("nothing scripted for {line}"),
            });
        };
        assert!(line.ends_with(&expected), "{line} is not {expected}");
        if status == UNCONNECTED {
            return Err(HttpError::Unconnected { reason: body });
        }
        if body.len() > request.most {
            return Err(HttpError::Larger { most: request.most });
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
