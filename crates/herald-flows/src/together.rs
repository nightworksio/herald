use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

/// What each of `futures` comes to, in their order, all of them awaited at
/// once.
pub(crate) async fn together<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut pending: Vec<Option<Pin<Box<F>>>> = futures
        .into_iter()
        .map(|future| Some(Box::pin(future)))
        .collect();
    let mut done: Vec<Option<F::Output>> = pending.iter().map(|_| None).collect();
    poll_fn(|context| {
        let mut waiting = false;
        for (slot, output) in pending.iter_mut().zip(done.iter_mut()) {
            let Some(future) = slot else {
                continue;
            };
            match future.as_mut().poll(context) {
                Poll::Ready(value) => {
                    *output = Some(value);
                    *slot = None;
                }
                Poll::Pending => waiting = true,
            }
        }
        if waiting {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    done.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    /// A future that waits `left` polls before it is ready with `value`,
    /// waking itself each time.
    struct Turns<'a> {
        left: u32,
        value: u32,
        polled: &'a Cell<u32>,
    }

    impl Future for Turns<'_> {
        type Output = u32;

        fn poll(mut self: Pin<&mut Self>, context: &mut std::task::Context<'_>) -> Poll<u32> {
            self.polled.set(self.polled.get() + 1);
            if self.left == 0 {
                return Poll::Ready(self.value);
            }
            self.left -= 1;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[test]
    fn every_future_is_awaited_at_once_and_answers_in_its_order() {
        let polled = Cell::new(0);
        let futures = vec![
            Turns {
                left: 2,
                value: 1,
                polled: &polled,
            },
            Turns {
                left: 0,
                value: 2,
                polled: &polled,
            },
            Turns {
                left: 1,
                value: 3,
                polled: &polled,
            },
        ];
        assert_eq!(pollster::block_on(together(futures)), [1, 2, 3]);
        assert_eq!(polled.get(), 6);
    }

    #[test]
    fn no_futures_come_to_nothing() {
        assert_eq!(
            pollster::block_on(together(Vec::<Turns<'_>>::new())),
            [0_u32; 0]
        );
    }
}
