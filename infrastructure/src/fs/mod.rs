//! Document files: metadata and fingerprints, the size policy, loading, reading for a tab, safe
//! saving, and the open and save pipeline over the encoding module.

mod document;
pub(crate) mod durable;
mod load;
mod meta;
mod read;
mod rename;
mod save;
mod size;

pub use document::{
    DecodeReport, EncodingChoice, OpenedDocument, open_document, open_document_as, save_document,
    text_chunks,
};
pub use load::{LoadError, load};
pub use meta::{FileMeta, file_meta, fingerprint, fingerprint_of};
pub use read::{DocInfo, ReadFile, read_file};
pub use rename::rename_no_replace;
pub use save::{SaveError, SaveMethod, SaveOptions, SaveOutcome, remove_stale_temps, safe_save};
pub use size::{SizeClass, SizePolicy};
