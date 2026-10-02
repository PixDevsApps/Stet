//! One file of a Replace in Files run: read, decode, replace in the LF text with the
//! in-document engine, write the replacements back into the file's own bytes, check, save.

use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use encoding_rs::Encoding;
use stet_domain::replace_files::{LfText, RawEdit, Skip, apply_raw, raw_edits};
use stet_domain::search::Template;

use crate::encoding::{bom_bytes, decode, detect, encode};
use crate::fs::{
    LoadError, SaveError, SaveMethod, SaveOptions, SizePolicy, fingerprint, load, safe_save,
};
use crate::search::{Hit, Match, Matcher, SearchError, document_hits};

/// How many unmappable characters a skip lists.
const LISTED_UNMAPPABLE: usize = 5;

/// The new content of a file, worked out but not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The bytes to write; empty when nothing changes.
    pub bytes: Vec<u8>,
    /// Matches replaced, those whose replacement equals the match included.
    pub replacements: usize,
    /// Whether `bytes` differ from the file's: some replacement differs from its match.
    pub changed: bool,
    /// The replaced text's lines in the new text, at most the limit given.
    pub hits: Vec<Hit>,
    pub encoding: &'static Encoding,
}

/// Why a plan could not be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// The file matched this many times, but is left alone for the reason given.
    Skip(Skip, usize),
    Search(SearchError),
}

/// Works out a file's new bytes: `bytes` decoded as the editor would open them (detection,
/// BOM, NUL as U+2400), searched in their LF text with `matcher`, and each replacement written
/// back into the file's own text and encoded on its own, while every byte between them is
/// copied from `bytes`. `None` when nothing matches, so files without a match are never
/// reported, whatever they are.
///
/// Refused, with the reason and the matches: binary files, decodes that are lossy or met
/// malformed bytes, replacements the encoding cannot represent, a replacement that would join
/// a lone CR and a lone LF into one line break, and any case where the untouched text would
/// not come back as the same bytes (a stateful encoding).
pub fn plan(
    bytes: &[u8],
    matcher: &Matcher,
    template: &Template,
    cancel: &AtomicBool,
    hit_limit: usize,
) -> Result<Option<Plan>, PlanError> {
    let detection = detect(bytes);
    let (encoding, bom) = (detection.encoding, detection.bom);
    let name = || encoding.name().to_owned();
    let decoded = decode(bytes, encoding, bom);
    let lf = LfText::new(&decoded.text);
    let result = matcher
        .replace_all(&lf.text, template, None, cancel)
        .map_err(PlanError::Search)?;
    if result.count == 0 {
        return Ok(None);
    }
    let skip = |reason| PlanError::Skip(reason, result.count);
    if detection.binary {
        return Err(skip(Skip::Binary));
    }
    if decoded.had_errors || decoded.lossy_roundtrip {
        return Err(skip(Skip::Inexact { encoding: name() }));
    }
    if result.edits.is_empty() {
        return Ok(Some(Plan {
            bytes: Vec::new(),
            replacements: result.count,
            changed: false,
            hits: Vec::new(),
            encoding,
        }));
    }
    let ranges = result.byte_ranges(&lf.text);
    let edits = raw_edits(
        &lf,
        ranges
            .iter()
            .cloned()
            .zip(result.edits.iter().map(|edit| edit.insert.as_str())),
    );
    let new_raw =
        apply_raw(&decoded.text, &edits).ok_or_else(|| skip(Skip::LineBreaksWouldJoin))?;
    let map_placeholder = decoded.map_placeholder();
    let head = if bom { bom_bytes(encoding).len() } else { 0 };
    let out = splice(
        &bytes[..head],
        &bytes[head..],
        &decoded.text,
        &edits,
        encoding,
        map_placeholder,
    )
    .map_err(skip)?;
    // The result must read back as the new text, in the same encoding, without errors.
    let check = decode(&out, encoding, bom);
    if check.had_errors || check.text != new_raw || detect(&out).bom != bom {
        return Err(skip(Skip::BytesNotKept { encoding: name() }));
    }
    let new_text = result.apply(&lf.text);
    let hits = replaced_hits(&new_text, &ranges, &result.edits, hit_limit);
    Ok(Some(Plan {
        bytes: out,
        replacements: result.count,
        changed: true,
        hits,
        encoding,
    }))
}

/// `head` (the BOM), then `content` with each edit's span replaced by its encoded insert.
/// Each stretch of `raw` before and inside an edit is encoded to find how many bytes of
/// `content` it took; they must be exactly those bytes, which are then copied.
fn splice(
    head: &[u8],
    content: &[u8],
    raw: &str,
    edits: &[RawEdit],
    encoding: &'static Encoding,
    map_placeholder: bool,
) -> Result<Vec<u8>, Skip> {
    let not_kept = || Skip::BytesNotKept {
        encoding: encoding.name().to_owned(),
    };
    let mut out = Vec::with_capacity(head.len() + content.len() + content.len() / 16);
    out.extend_from_slice(head);
    let mut consumed = 0;
    let mut take = |text: &str, copy: bool, out: &mut Vec<u8>| -> Result<(), Skip> {
        if text.is_empty() {
            return Ok(());
        }
        let encoded = encode(text, encoding, false, map_placeholder).map_err(|_| not_kept())?;
        let end = consumed + encoded.len();
        if content.get(consumed..end) != Some(encoded.as_slice()) {
            return Err(not_kept());
        }
        if copy {
            out.extend_from_slice(&content[consumed..end]);
        }
        consumed = end;
        Ok(())
    };
    let mut unmappable: Vec<char> = Vec::new();
    let mut unmappable_count = 0;
    let mut kept = 0;
    for edit in edits {
        take(&raw[kept..edit.range.start], true, &mut out)?;
        take(&raw[edit.range.clone()], false, &mut out)?;
        match encode(&edit.insert, encoding, false, map_placeholder) {
            Ok(bytes) => out.extend_from_slice(&bytes),
            Err(error) => {
                unmappable_count += error.count;
                for (_, character) in error.chars {
                    if unmappable.len() < LISTED_UNMAPPABLE && !unmappable.contains(&character) {
                        unmappable.push(character);
                    }
                }
            }
        }
        kept = edit.range.end;
    }
    take(&raw[kept..], true, &mut out)?;
    if unmappable_count > 0 {
        return Err(Skip::Unmappable {
            encoding: encoding.name().to_owned(),
            chars: unmappable,
            count: unmappable_count,
        });
    }
    if consumed != content.len() {
        return Err(not_kept());
    }
    Ok(out)
}

/// The lines of the replaced text in `new_text`, the LF text after the edits, for at most
/// `limit` edits. `ranges` are the edits' byte ranges in the text before
/// ([`crate::search::ReplaceAll::byte_ranges`]).
pub fn replaced_hits(
    new_text: &str,
    ranges: &[std::ops::Range<usize>],
    edits: &[stet_domain::text::TextEdit],
    limit: usize,
) -> Vec<Hit> {
    let bytes = new_text.as_bytes();
    let mut matches = Vec::with_capacity(edits.len().min(limit));
    let mut shift: isize = 0;
    let (mut line, mut line_from) = (0, 0);
    let (mut chars, mut chars_from) = (0, 0);
    for (range, edit) in ranges.iter().zip(edits).take(limit) {
        let start = (range.start as isize + shift) as usize;
        let end = start + edit.insert.len();
        shift += edit.insert.len() as isize - (range.end - range.start) as isize;
        line += memchr::memchr_iter(b'\n', &bytes[line_from..start]).count();
        chars += new_text[chars_from..start].chars().count();
        let length = edit.insert.chars().count();
        matches.push(Match {
            start: chars,
            end: chars + length,
            line,
        });
        // The next search for line breaks starts after this replacement.
        line += memchr::memchr_iter(b'\n', &bytes[start..end]).count();
        line_from = end;
        chars += length;
        chars_from = end;
    }
    // A replacement that spans lines reports where it starts.
    document_hits(new_text, &matches)
}

/// How one file of a run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOutcome {
    /// Nothing matched.
    NoMatch,
    /// The replacements were made; `written` is false when every one equalled its match.
    Replaced {
        replacements: usize,
        written: bool,
        hits: Vec<Hit>,
        method: Option<SaveMethod>,
    },
    /// Left alone, with the matches found when they were counted.
    Skipped {
        reason: Skip,
        matches: Option<usize>,
    },
    /// Reading or writing failed.
    Failed(String),
    /// The run was stopped while this file was searched; nothing was written.
    Cancelled,
}

/// Where a test stops [`replace_file`] to change the file under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The new bytes are worked out; the file's fingerprint is checked next.
    Planned,
}

/// What every file of a run shares.
#[derive(Debug, Clone)]
pub struct FileOptions {
    /// Files from this size are skipped.
    pub max_bytes: u64,
    /// Where an in-place save keeps the previous content while it writes ([`safe_save`]).
    pub save: SaveOptions,
    /// How many replaced lines a file reports at most.
    pub hit_limit: usize,
}

/// Replaces in the file `path` and writes it with [`safe_save`], keeping its symbolic link,
/// mode and owner; a file that changed since it was read is left alone.
pub fn replace_file(
    path: &Path,
    matcher: &Matcher,
    template: &Template,
    options: &FileOptions,
    cancel: &AtomicBool,
) -> FileOutcome {
    replace_file_with_hook(path, matcher, template, options, cancel, &mut |_| {})
}

pub(crate) fn replace_file_with_hook(
    path: &Path,
    matcher: &Matcher,
    template: &Template,
    options: &FileOptions,
    cancel: &AtomicBool,
    hook: &mut dyn FnMut(Stage),
) -> FileOutcome {
    let size = match std::fs::metadata(path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return FileOutcome::NoMatch,
        Err(error) => return FileOutcome::Failed(error.to_string()),
    };
    if size >= options.max_bytes {
        return FileOutcome::Skipped {
            reason: Skip::TooLarge {
                size,
                limit: options.max_bytes,
            },
            matches: None,
        };
    }
    let loaded = match load(path, &SizePolicy::default()) {
        Ok(loaded) => loaded,
        Err(LoadError::NotFound { .. }) => return FileOutcome::NoMatch,
        Err(error) => return FileOutcome::Failed(error.to_string()),
    };
    let plan = match plan(&loaded.bytes, matcher, template, cancel, options.hit_limit) {
        Ok(Some(plan)) => plan,
        Ok(None) => return FileOutcome::NoMatch,
        Err(PlanError::Skip(reason, matches)) => {
            return FileOutcome::Skipped {
                reason,
                matches: Some(matches),
            };
        }
        Err(PlanError::Search(SearchError::Cancelled)) => return FileOutcome::Cancelled,
        Err(PlanError::Search(error)) => return FileOutcome::Failed(error.to_string()),
    };
    drop(loaded.bytes);
    if !plan.changed {
        return FileOutcome::Replaced {
            replacements: plan.replacements,
            written: false,
            hits: plan.hits,
            method: None,
        };
    }
    let skipped = |reason| FileOutcome::Skipped {
        reason,
        matches: Some(plan.replacements),
    };
    if !loaded.meta.writable {
        return skipped(Skip::ReadOnly);
    }
    hook(Stage::Planned);
    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        return FileOutcome::Cancelled;
    }
    match fingerprint(path) {
        Ok(now) if loaded.meta.fingerprint().matches(&now) => {}
        _ => return skipped(Skip::ChangedMeanwhile),
    }
    match safe_save(path, &plan.bytes, &options.save) {
        Ok(outcome) => FileOutcome::Replaced {
            replacements: plan.replacements,
            written: true,
            hits: plan.hits,
            method: Some(outcome.method),
        },
        Err(SaveError::PermissionDenied { .. } | SaveError::ReadOnlyFilesystem { .. }) => {
            skipped(Skip::ReadOnly)
        }
        Err(error) => FileOutcome::Failed(error.to_string()),
    }
}
