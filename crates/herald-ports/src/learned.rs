use std::sync::OnceLock;

/// What a destination learns from its platform once, such as whom its
/// token names, kept for as long as the destination is. Two are equal
/// whatever they hold: what is learned says nothing of how a destination is
/// set.
#[derive(Debug, Clone)]
pub struct Learned<T>(OnceLock<T>);

impl<T> Learned<T> {
    /// Nothing learned yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(OnceLock::new())
    }

    /// What was learned, where it was.
    #[must_use]
    pub fn get(&self) -> Option<&T> {
        self.0.get()
    }

    /// What was learned: `value`, unless something was learned first.
    pub fn learn(&self, value: T) -> &T {
        self.0.get_or_init(|| value)
    }
}

impl<T> Default for Learned<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> PartialEq for Learned<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<T> Eq for Learned<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_value_learned_is_kept_and_any_two_are_equal() {
        let learned = Learned::default();
        assert_eq!(learned.get(), None);
        assert_eq!(learned.learn(1), &1);
        assert_eq!(learned.learn(2), &1);
        assert_eq!(learned.get(), Some(&1));
        assert_eq!(learned, Learned::new());
    }
}
