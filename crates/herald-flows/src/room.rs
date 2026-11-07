use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A number of places for work done at once, such as `[server] jobs`
/// (ADR-0014).
#[derive(Debug)]
pub struct Room {
    held: Arc<AtomicUsize>,
    most: usize,
}

impl Room {
    /// `most` places, none held.
    #[must_use]
    pub fn new(most: usize) -> Self {
        Self {
            held: Arc::new(AtomicUsize::new(0)),
            most,
        }
    }

    /// A place for one more piece of work, where the room has one.
    #[must_use]
    pub fn place(&self) -> Option<Place> {
        self.held
            .try_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                (held < self.most).then_some(held + 1)
            })
            .ok()
            .map(|_| Place(Arc::clone(&self.held)))
    }
}

/// A place held in the room, given up when it is dropped.
#[derive(Debug)]
pub struct Place(Arc<AtomicUsize>);

impl Drop for Place {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A round marked running until it is dropped, so one of each kind runs
/// at a time (ADR-0014).
pub struct Running<'r>(&'r AtomicBool);

impl<'r> Running<'r> {
    /// `running` marked, where it was not already.
    #[must_use]
    pub fn start(running: &'r AtomicBool) -> Option<Self> {
        (!running.swap(true, Ordering::AcqRel)).then_some(Self(running))
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_held_until_it_is_dropped() {
        let room = Room::new(2);
        let first = room.place();
        let second = room.place();
        assert!(first.is_some() && second.is_some());
        assert!(room.place().is_none());
        drop(first);
        assert!(room.place().is_some());
        assert!(Room::new(0).place().is_none());
    }

    #[test]
    fn a_round_runs_once_at_a_time_and_again_once_it_is_done() {
        let running = AtomicBool::new(false);
        let first = Running::start(&running);
        assert!(first.is_some());
        assert!(Running::start(&running).is_none());
        drop(first);
        assert!(Running::start(&running).is_some());
    }
}
