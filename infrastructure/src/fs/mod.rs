//! Document files: metadata and fingerprints, the size policy, loading, safe saving, and the
//! open and save pipeline over the encoding module.

mod document;
mod load;
mod meta;
mod rename;
mod save;
mod size;

pub use document::{
    DecodedDocument, DocumentFormat, EncodingChoice, OpenedDocument, decode_document,
    encode_document, open_document, open_document_as, save_document, text_chunks,
};
pub use load::{LoadError, Loaded, load};
pub use meta::{FileMeta, Fingerprint, file_meta, fingerprint, fingerprint_of};
pub use rename::rename_no_replace;
pub use save::{SaveError, SaveMethod, SaveOptions, SaveOutcome, remove_stale_temps, safe_save};
pub use size::{MIB, Refusal, SizeClass, SizePolicy, mem_available};
