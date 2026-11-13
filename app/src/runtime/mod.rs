//! The GTK thread's async plumbing, shared by every feature: jobs on GLib's thread pool,
//! tokens that let only the newest of overlapping runs apply its result, debounced callbacks,
//! notifications to wait on instead of polling, listeners to call on a change, signal handlers that run once, and flags that
//! tell signal handlers what the window itself is doing.

mod debounce;
mod flag;
mod latest;
mod listeners;
mod notify;
mod signal;
pub mod worker;

pub use debounce::Debounce;
pub use flag::{Flag, Raised};
pub use latest::{Latest, SharedLatest, Stamp, Ticket};
pub use listeners::Listeners;
pub use notify::{Notify, wait_until};
pub use signal::connect_once;
