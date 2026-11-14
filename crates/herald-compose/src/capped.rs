use std::num::NonZeroUsize;

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
    pub fn new(http: H, most: NonZeroUsize) -> Self {
        Self {
            http,
            permits: Semaphore::new(most.get()),
        }
    }
}

impl<H: Http> Http for Capped<H> {
    async fn send(&self, request: Request) -> Result<Response, HttpError> {
        let _permit = self.permits.acquire().await;
        self.http.send(request).await
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::Poll;

    use herald_ports::{Method, SecretText};

    use super::*;

    #[derive(Default)]
    struct Counting {
        now: AtomicUsize,
        most: AtomicUsize,
    }

    impl Http for Counting {
        async fn send(&self, _: Request) -> Result<Response, HttpError> {
            let now = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.most.fetch_max(now, Ordering::SeqCst);
            let mut yielded = false;
            poll_fn(|context| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok(Response {
                status: 202,
                headers: Vec::new(),
                body: Vec::new(),
            })
        }
    }

    fn sent(most: usize) -> usize {
        let capped = Capped::new(Counting::default(), NonZeroUsize::new(most).unwrap());
        let request = || {
            Request::new(
                Method::Get,
                SecretText::plain("https://social.example/users/ana"),
            )
        };
        let answers = pollster::block_on(futures_util::future::join3(
            capped.send(request()),
            capped.send(request()),
            capped.send(request()),
        ));
        assert!(
            [answers.0, answers.1, answers.2]
                .iter()
                .all(|answer| answer.as_ref().is_ok_and(|response| response.status == 202))
        );
        capped.http.most.load(Ordering::SeqCst)
    }

    #[test]
    fn a_capped_http_sends_no_more_at_once_than_it_is_allowed() {
        assert_eq!(sent(1), 1);
        assert_eq!(sent(2), 2);
        assert_eq!(sent(3), 3);
    }
}
