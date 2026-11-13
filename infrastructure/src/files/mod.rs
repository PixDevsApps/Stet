//! Work over a folder's files: Find in Files ([`find`]), Replace in Files ([`replace`]) and
//! quick open's project walk ([`project`]).
//!
//! What they share lives here once: the walk and its options (hidden files, `.gitignore`,
//! subfolders, filters; symbolic links are not followed), the job a search or a replace runs
//! as ([`JobHandle`], cancelled through a [`CancelToken`]), the index of open documents the
//! walk meets, and the errors.

mod error;
mod filters;
pub mod find;
mod job;
mod open;
pub mod project;
pub mod replace;
mod walk;

pub use error::{FileError, PathError, StartError};
pub use job::{CancelToken, JobHandle};
