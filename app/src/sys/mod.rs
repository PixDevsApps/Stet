//! The only unsafe code in Stet: small safe wrappers over the GTK and GtkSourceView C calls that
//! the bindings lack or get wrong. Each wrapper is a safe function whose unsafe blocks carry
//! their reasoning, so nothing outside this module needs `unsafe`.
//!
//! - [`search`]: GtkSourceView's asynchronous Find Next and Find Previous, which honour "no
//!   match".
//! - [`source_marks`]: moving an iterator to the next source mark.
//! - [`text_buffer`]: an `insert-text` handler that may take over an insertion.
//! - [`accessible_text`]: a NULL-safe `GtkAccessibleText.get_selection` for the editor's view.

pub(crate) mod accessible_text;
pub(crate) mod search;
pub(crate) mod source_marks;
pub(crate) mod text_buffer;
