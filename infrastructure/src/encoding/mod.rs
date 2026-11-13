//! Stet's own encoding pipeline (ADR-007): detection, chunked decoding with the NUL
//! placeholder and the lossy-round-trip check, and strict encoding.

mod decode;
mod detect;
mod encode;
#[cfg(test)]
pub(crate) mod fixtures;
mod table;

pub use decode::{decode, decode_to_lf};
pub use detect::{Detection, DetectionSource, detect};
pub use encode::{DocumentFormat, Unmappable, check_encodable, encode};
pub use encoding_rs::Encoding;
pub use table::{
    ENCODINGS, EncodingEntry, encoding_for_name, entry_for, entry_for_id, status_label,
};

use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE};
use stet_domain::text::NUL_PLACEHOLDER;

/// [`NUL_PLACEHOLDER`] in UTF-8.
const PLACEHOLDER_UTF8: &[u8] = &{
    let mut bytes = [0; NUL_PLACEHOLDER.len_utf8()];
    NUL_PLACEHOLDER.encode_utf8(&mut bytes);
    bytes
};

/// The byte order mark written for `encoding`; empty for encodings without one.
#[must_use]
pub fn bom_bytes(encoding: &'static Encoding) -> &'static [u8] {
    if encoding == UTF_8 {
        b"\xEF\xBB\xBF"
    } else if encoding == UTF_16LE {
        b"\xFF\xFE"
    } else if encoding == UTF_16BE {
        b"\xFE\xFF"
    } else {
        b""
    }
}

fn is_utf16(encoding: &'static Encoding) -> bool {
    encoding == UTF_16LE || encoding == UTF_16BE
}
