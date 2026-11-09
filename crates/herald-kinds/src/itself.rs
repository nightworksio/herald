use std::any::Any;

/// A value a trait object reads back and compares as its own type.
pub trait Itself: Any {
    /// Itself, for code that reads one kind's own type.
    fn as_any(&self) -> &dyn Any;

    /// Whether `other` is a value of its type equal to it.
    fn same(&self, other: &dyn Any) -> bool;
}

impl<T: PartialEq + Any> Itself for T {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>() == Some(self)
    }
}
