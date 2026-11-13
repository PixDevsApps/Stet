//! Standard-library I/O for the editor. No GTK; testable headless.

#![warn(unreachable_pub, missing_docs)]
#![warn(clippy::must_use_candidate)]

pub mod desktop;
pub mod encoding;
pub mod files;
pub mod fs;
pub mod omarchy;
pub mod recent;
pub mod sandbox;
pub mod search;
pub mod session_store;
pub mod settings_files;
pub mod style_cache;
pub mod updates;
pub mod xdg;

#[cfg(test)]
mod test_support {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// Serializes tests that write or spawn executables. A child forked by one test inherits
    /// another test's still-open write handle until it execs, and executing that freshly
    /// written script then fails with ETXTBSY.
    pub(crate) fn spawn_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
