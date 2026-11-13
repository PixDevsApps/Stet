//! The GTK thread's async plumbing, shared by every feature: jobs on GLib's thread pool,
//! tokens that let only the newest of overlapping runs apply its result, debounced callbacks,
//! notifications to wait on instead of polling, and signal handlers that run once.

mod debounce;
mod latest;
mod notify;
mod signal;
pub mod worker;

pub use debounce::Debounce;
pub use latest::{Latest, Ticket};
pub use notify::{Notify, wait_until};
pub use signal::connect_once;
