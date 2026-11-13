//! Blocking work off the GTK thread: file I/O and subprocesses run on a plain thread, and the
//! result comes back through an async channel awaited on the GTK main loop.

/// Runs `job` on a new thread. Resolves on the GTK thread with its result, or `None` if the
/// job panicked.
pub async fn run<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (sender, receiver) = async_channel::bounded(1);
    #[cfg(feature = "selftest")]
    let _pending = probe::Pending::start();
    let spawned = std::thread::Builder::new()
        .name("stet-worker".into())
        .spawn(move || {
            let _ = sender.send_blocking(job());
        });
    match spawned {
        Ok(_) => receiver.recv().await.ok(),
        Err(error) => {
            tracing::error!(%error, "could not start a worker thread");
            None
        }
    }
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
