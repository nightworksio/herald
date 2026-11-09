use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use crate::Itself;

/// A route's own options for its destination's kind, read: where and how
/// it announces there.
pub trait Placement: fmt::Debug + Send + Sync + Itself {
    /// Whether every post opens a thread of its own, as a Discord forum's
    /// does, so the route takes `thread` whatever its destination's
    /// features say.
    fn threads_each_post(&self) -> bool {
        false
    }

    /// The channel or room the route announces in, where its destination
    /// names one, as a command given there names it.
    fn place(&self) -> Option<&str> {
        None
    }

    /// The opt-in role members subscribe to, which the route mentions with
    /// its announcements.
    fn opt_in_role(&self) -> Option<String> {
        None
    }
}

/// A route's options for its destination, of any kind.
#[derive(Clone)]
pub struct Placed(Arc<dyn Placement>);

impl Placed {
    /// `options`, as a route's.
    #[must_use]
    pub fn new(options: impl Placement) -> Self {
        Self(Arc::new(options))
    }

    /// The options as their own kind's, where they are a `T`.
    #[must_use]
    pub fn of<T: Placement>(&self) -> Option<&T> {
        self.0.as_any().downcast_ref()
    }
}

impl Deref for Placed {
    type Target = dyn Placement;

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl fmt::Debug for Placed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl PartialEq for Placed {
    fn eq(&self, other: &Self) -> bool {
        self.0.same(other.0.as_any())
    }
}

impl Eq for Placed {}
