//! What a destination answered the request that makes what a step sends,
//! so a refusal that left nothing behind is told from one that may have
//! (ADR-0067).

use std::sync::OnceLock;

use herald_core::status::{BAD_REQUEST, REQUEST_TIMEOUT, SERVER_ERROR, TOO_MANY_REQUESTS};
use herald_ports::{Http, HttpError, Method, Request, Response};

/// An `Http` keeping the answer to the first request it passed on that
/// is not a read: the one that makes what the step sends.
pub(super) struct Heard<'a, H> {
    http: &'a H,
    making: OnceLock<Option<Response>>,
}

impl<'a, H: Http> Heard<'a, H> {
    /// `http`, the answer to its first request that is not a read kept.
    pub(super) const fn new(http: &'a H) -> Self {
        Self {
            http,
            making: OnceLock::new(),
        }
    }

    /// Whether the destination itself declined the request that makes
    /// what the step sends, so it made nothing: a client error that is
    /// neither a timeout nor a request to wait, answered as JSON that
    /// `envelope` says is the destination's own error envelope.
    pub(super) fn declined(&self, envelope: impl FnOnce(&Response) -> bool) -> bool {
        self.making
            .get()
            .and_then(Option::as_ref)
            .is_some_and(|answer| {
                (BAD_REQUEST..SERVER_ERROR).contains(&answer.status)
                    && !matches!(answer.status, REQUEST_TIMEOUT | TOO_MANY_REQUESTS)
                    && answer.is_json()
                    && envelope(answer)
            })
    }
}

impl<H: Http> Http for Heard<'_, H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let reads = request.method == Method::Get;
        let answer = self.http.send(request).await;
        if !reads {
            self.making.get_or_init(|| answer.as_ref().ok().cloned());
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use herald_core::media::JSON;
    use herald_ports::SecretText;

    use super::*;

    /// An `Http` answering each request with the next status, as JSON.
    struct Answers(Mutex<VecDeque<u16>>);

    #[allow(
        clippy::unused_async_trait_impl,
        reason = "a fake answers at once, and is written as the async trait it fakes"
    )]
    impl Http for Answers {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            let status = self.0.lock().unwrap().pop_front();
            status.map_or(Err(HttpError::TimedOut), |status| {
                Ok(Response {
                    status,
                    headers: vec![("Content-Type".to_owned(), JSON.to_owned())],
                    body: b"{}".to_vec(),
                })
            })
        }
    }

    /// Whether a step sending `methods`, answered `statuses` in turn, was
    /// declined.
    fn declined(methods: &[Method], statuses: &[u16]) -> bool {
        let answers = Answers(Mutex::new(statuses.iter().copied().collect()));
        let heard = Heard::new(&answers);
        for method in methods {
            let request = Request::new(*method, SecretText::plain("https://place.example"));
            drop(pollster::block_on(heard.send(request)));
        }
        heard.declined(|_| true)
    }

    #[test]
    fn only_the_first_request_that_is_not_a_read_tells_whether_the_step_was_declined() {
        assert!(declined(&[Method::Put], &[403]));
        assert!(declined(&[Method::Get, Method::Put], &[404, 403]));
        assert!(!declined(&[Method::Get, Method::Put], &[403, 200]));
        assert!(!declined(&[Method::Put, Method::Put], &[200, 403]));
        assert!(declined(&[Method::Post, Method::Delete], &[400, 200]));
        assert!(!declined(&[Method::Put, Method::Put], &[]));
        assert!(!declined(&[Method::Put], &[408]));
        assert!(!declined(&[Method::Put], &[429]));
        assert!(!declined(&[Method::Put], &[502]));
        assert!(!declined(&[Method::Get], &[403]));
    }

    #[test]
    fn a_client_error_is_declined_only_as_json_its_destination_says_is_its_own() {
        let answers = Answers(Mutex::new([403].into()));
        let heard = Heard::new(&answers);
        let request = Request::new(Method::Post, SecretText::plain("https://place.example"));
        drop(pollster::block_on(heard.send(request)));
        assert!(!heard.declined(|_| false));
        assert!(heard.declined(|answer| answer.body == b"{}"));
    }
}
