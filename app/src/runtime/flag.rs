//! Flags that tell signal handlers an operation of the window's own is under way, such as a
//! tab moving to the other view, whose detach is then not a close.

use std::cell::Cell;

/// A flag that is up while a [`Raised`] guard from [`Flag::raise`] lives.
#[derive(Debug, Default)]
pub struct Flag(Cell<bool>);

impl Flag {
    /// Raises the flag until the guard drops. The guard lowers it even when an outer guard is
    /// still alive: the flag says an operation is running, not how many are.
    #[must_use = "the flag lowers as soon as the guard drops"]
    pub fn raise(&self) -> Raised<'_> {
        self.0.set(true);
        Raised(&self.0)
    }

    pub fn is_raised(&self) -> bool {
        self.0.get()
    }
}

/// Lowers its [`Flag`] when dropped.
#[derive(Debug)]
pub struct Raised<'a>(&'a Cell<bool>);

impl Drop for Raised<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

#[cfg(test)]
mod tests {
    use super::Flag;

    #[test]
    fn the_flag_is_up_while_the_guard_lives() {
        let flag = Flag::default();
        assert!(!flag.is_raised());
        {
            let _raised = flag.raise();
            assert!(flag.is_raised());
        }
        assert!(!flag.is_raised());
    }

    #[test]
    fn a_nested_guard_lowers_the_flag() {
        let flag = Flag::default();
        let _outer = flag.raise();
        drop(flag.raise());
        assert!(!flag.is_raised());
    }
}
