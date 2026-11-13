//! Blocking work off the GTK thread: file I/O and subprocesses run on GLib's thread pool, and
//! the result comes back on the GTK main loop.

use gtk4::gio;

/// Runs `job` on GLib's thread pool ([`gio::spawn_blocking`]). Resolves on the GTK thread with
/// its result, or `None` if the job panicked.
///
/// The pool is GIO's, shared with its own async I/O: it starts at ten threads and adds more
/// while jobs wait. No job waits for another job (the session store's flush waits for its own
/// writer thread, its lock for another process), so a busy pool only delays work, it cannot
/// deadlock.
pub async fn run<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    #[cfg(feature = "selftest")]
    let _pending = probe::Pending::start();
    gio::spawn_blocking(job).await.ok()
}

#[cfg(feature = "selftest")]
pub use probe::pending;

/// The self-test waits until no job is pending.
#[cfg(feature = "selftest")]
mod probe {
    use std::cell::Cell;

    thread_local! {
        static PENDING: Cell<usize> = const { Cell::new(0) };
    }

    /// Counts a job as pending until dropped, also when its future is dropped before the
    /// result.
    pub(super) struct Pending;

    impl Pending {
        pub(super) fn start() -> Self {
            PENDING.with(|pending| pending.set(pending.get() + 1));
            Self
        }
    }

    impl Drop for Pending {
        fn drop(&mut self) {
            PENDING.with(|pending| pending.set(pending.get() - 1));
        }
    }

    /// Jobs started on this thread whose results have not arrived yet.
    pub fn pending() -> usize {
        PENDING.with(Cell::get)
    }
}
