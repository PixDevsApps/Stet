//! The window's documents on disk and in the session: opening and saving files ([`files`]),
//! changes other programs make to them ([`disk`]), encodings and line endings ([`encoding`]),
//! the banners a document's state calls for ([`banners`]), and the session that restores the
//! tabs and backs up unsaved changes ([`session`]).

pub(super) mod banners;
pub(super) mod disk;
pub(super) mod encoding;
pub(super) mod files;
pub(super) mod session;
