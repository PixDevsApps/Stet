//! Our own search engine over text snapshots (ADR-005 amendment): PCRE2 through the `pcre2`
//! crate, compiled like GtkSourceView compiles the same translated pattern, so the matches it
//! replaces are the ones the widget highlights.
//!
//! Every replace runs here: [`Matcher::replace_all`] and [`Matcher::replace_one`] return
//! [`stet_domain::text::TextEdit`]s in character offsets, which the app applies in one user
//! action, or as one bulk edit above [`BULK_EDIT_THRESHOLD`] edits (ADR-003 amendment). The
//! engine also counts, highlights and navigates the patterns GtkSourceView cannot
//! ([`Matcher::needs_own_matcher`]).
//!
//! Everything here works on `&str` snapshots and may run on any thread; long operations take
//! an `AtomicBool` to cancel them.

mod hits;
mod matcher;
pub mod parity;
mod replace;
mod text;

#[cfg(test)]
mod tests;

pub use hits::{Hit, MAX_LINE_CHARS, document_hits};
pub use matcher::{FindAll, Match, Matcher, NextMatch, PatternError, SearchError};
pub use replace::{BULK_EDIT_THRESHOLD, ReplaceAll, replace_all, replace_one};

pub(crate) use hits::make_hit;
pub(crate) use matcher::{JIT_STACK, pcre2_message};
pub(crate) use text::next_boundary;
