//! Blocking work off the GTK thread: file I/O and subprocesses run on a plain thread, and the
//! result comes back through an async channel awaited on the GTK main loop.

use std::cell::Cell;

thread_local! {
    static PENDING: Cell<usize> = const { Cell::new(0) };
}

/// Runs `job` on a new thread. Resolves on the GTK thread with its result, or `None` if the
/// job panicked.
pub async fn run<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (sender, receiver) = async_channel::bounded(1);
    PENDING.with(|pending| pending.set(pending.get() + 1));
    let spawned = std::thread::Builder::new()
        .name("stet-worker".into())
        .spawn(move || {
            let _ = sender.send_blocking(job());
        });
    let result = match spawned {
        Ok(_) => receiver.recv().await.ok(),
        Err(error) => {
            tracing::error!(%error, "could not start a worker thread");
            None
        }
    };
    PENDING.with(|pending| pending.set(pending.get() - 1));
    result
}

/// Jobs started on this thread whose results have not arrived yet.
pub fn pending() -> usize {
    PENDING.with(Cell::get)
}
