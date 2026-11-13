//! One pending callback on the GTK main loop, which a newer one replaces.

use gtk4::glib;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// A callback waiting on the main loop: scheduling another replaces it, and dropping the
/// `Debounce` removes it. For work that should run once things have been quiet for a moment,
/// or once per burst ([`is_pending`](Self::is_pending)).
#[derive(Debug, Default)]
pub struct Debounce {
    /// Shared with the callback, which empties it as it runs, so a source that has run is
    /// never removed.
    pending: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Debounce {
    /// Runs `callback` after `delay`, in place of the callback still waiting, if any.
    pub fn schedule(&self, delay: Duration, callback: impl FnOnce() + 'static) {
        self.cancel();
        let pending = Rc::downgrade(&self.pending);
        let source = glib::timeout_add_local_once(delay, move || {
            if let Some(pending) = pending.upgrade() {
                pending.replace(None);
            }
            callback();
        });
        self.pending.replace(Some(source));
    }

    /// Runs `callback` once the main loop is idle, in place of the callback still waiting, if
    /// any.
    pub fn schedule_idle(&self, callback: impl FnOnce() + 'static) {
        self.cancel();
        let pending = Rc::downgrade(&self.pending);
        let source = glib::idle_add_local_once(move || {
            if let Some(pending) = pending.upgrade() {
                pending.replace(None);
            }
            callback();
        });
        self.pending.replace(Some(source));
    }

    /// A callback is waiting.
    pub fn is_pending(&self) -> bool {
        self.pending.borrow().is_some()
    }

    /// Removes the waiting callback, if any.
    pub fn cancel(&self) {
        if let Some(source) = self.pending.take() {
            source.remove();
        }
    }
}

impl Drop for Debounce {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::{Mutex, MutexGuard};
    use std::time::Instant;

    static MAIN_CONTEXT: Mutex<()> = Mutex::new(());

    /// Owns GLib's default main context for one test: the tests run on several threads.
    pub(in crate::runtime) fn main_context() -> (MutexGuard<'static, ()>, glib::MainContext) {
        let guard = MAIN_CONTEXT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (guard, glib::MainContext::default())
    }

    /// Runs the main loop until `done`, or panics after two seconds.
    pub(in crate::runtime) fn iterate_until(context: &glib::MainContext, done: impl Fn() -> bool) {
        let started = Instant::now();
        while !done() {
            assert!(started.elapsed() < Duration::from_secs(2), "timed out");
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Runs the main loop for `duration`.
    fn iterate_for(context: &glib::MainContext, duration: Duration) {
        let started = Instant::now();
        while started.elapsed() < duration {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_newer_callback_replaces_the_waiting_one() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let runs = Rc::new(Cell::new(0));
        let debounce = Debounce::default();
        for value in [1, 2, 3] {
            let runs = runs.clone();
            debounce.schedule(Duration::from_millis(20), move || {
                runs.set(runs.get() + value);
            });
        }
        assert!(debounce.is_pending());
        iterate_until(&context, || !debounce.is_pending());
        iterate_for(&context, Duration::from_millis(40));
        assert_eq!(runs.get(), 3);
    }

    #[test]
    fn cancel_and_drop_remove_the_waiting_callback() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let runs = Rc::new(Cell::new(0));
        let debounce = Debounce::default();
        let counted = runs.clone();
        debounce.schedule(Duration::from_millis(5), move || {
            counted.set(counted.get() + 1);
        });
        debounce.cancel();
        assert!(!debounce.is_pending());
        let counted = runs.clone();
        debounce.schedule_idle(move || counted.set(counted.get() + 1));
        drop(debounce);
        iterate_for(&context, Duration::from_millis(30));
        assert_eq!(runs.get(), 0);
    }

    #[test]
    fn a_callback_may_schedule_the_next() {
        let (_guard, context) = main_context();
        let _owner = context.acquire().expect("the default main context");
        let debounce = Rc::new(Debounce::default());
        let runs = Rc::new(Cell::new(0));
        let weak = Rc::downgrade(&debounce);
        let counted = runs.clone();
        debounce.schedule_idle(move || {
            counted.set(counted.get() + 1);
            if let Some(debounce) = weak.upgrade() {
                debounce.schedule_idle(move || counted.set(counted.get() + 1));
            }
        });
        iterate_until(&context, || runs.get() == 2);
        assert!(!debounce.is_pending());
    }
}
