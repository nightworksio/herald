use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use herald_ports::{Http, HttpError, Request, Response};

/// An `Http` that counts each request it sends on a count it shares with
/// every other connection herald makes, so a host's limit on outbound
/// requests is kept.
#[derive(Debug, Clone)]
pub struct Counted<H> {
    http: H,
    made: Arc<AtomicUsize>,
}

impl<H> Counted<H> {
    /// `http`, counting each request on `made`.
    #[must_use]
    pub(crate) const fn new(http: H, made: Arc<AtomicUsize>) -> Self {
        Self { http, made }
    }
}

impl<H: Http> Http for Counted<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        self.made.fetch_add(1, Ordering::Relaxed);
        self.http.send(request).await
    }
}

#[cfg(test)]
mod tests {
    use herald_ports::{Method, SecretText};

    use super::*;

    struct Answering;

    impl Http for Answering {
        fn send(&self, _: Request) -> impl Future<Output = Result<Response, HttpError>> + Send {
            std::future::ready(Ok(Response {
                status: 204,
                headers: Vec::new(),
                body: Vec::new(),
            }))
        }
    }

    #[test]
    fn each_request_sent_is_counted_on_the_shared_count() {
        let made = Arc::new(AtomicUsize::new(0));
        let one = Counted::new(Answering, Arc::clone(&made));
        let other = Counted::new(Answering, Arc::clone(&made));
        let request = || Request::new(Method::Get, SecretText::plain("https://forge.example/"));
        let sent = pollster::block_on(async {
            [
                one.send(request()).await,
                other.send(request()).await,
                one.send(request()).await,
            ]
        });
        assert!(
            sent.iter()
                .all(|sent| sent.as_ref().is_ok_and(|response| response.status == 204))
        );
        assert_eq!(made.load(Ordering::Relaxed), 3);
    }
}
