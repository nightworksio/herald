use async_lock::Semaphore;
use herald_ports::{Http, HttpError, Request, Response};

/// An `Http` that sends at most so many requests at once, the rest waiting
/// their turn.
#[derive(Debug)]
pub struct Capped<H> {
    http: H,
    permits: Semaphore,
}

impl<H> Capped<H> {
    /// `http`, sending at most `most` requests at once.
    #[must_use]
    pub fn new(http: H, most: usize) -> Self {
        Self {
            http,
            permits: Semaphore::new(most),
        }
    }
}

impl<H: Http> Http for Capped<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let _permit = self.permits.acquire().await;
        self.http.send(request).await
    }
}
