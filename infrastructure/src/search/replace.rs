use std::borrow::Cow;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use stet_domain::search::Template;
use stet_domain::text::{BULK_EDIT_THRESHOLD, TextEdit};

use super::matcher::{Match, MatchCaptures, Matcher, Matches, Scope, SearchError};
use stet_domain::text::{ByteCursor, ByteOffset, CharCursor, CharOffset, OffsetRange};

/// The result of a Replace All: edits in character offsets of the searched text, in order,
/// never overlapping. Apply them in one user action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReplaceAll {
    pub edits: Vec<TextEdit>,
    /// Matches replaced. A replacement equal to the matched text counts but makes no edit.
    pub count: usize,
    /// The edits' byte ranges in the searched text, recorded while matching.
    spans: Vec<Range<ByteOffset>>,
    text_len: usize,
}

impl ReplaceAll {
    /// More edits than the buffer should take one by one.
    pub fn is_bulk(&self) -> bool {
        self.edits.len() > BULK_EDIT_THRESHOLD
    }

    /// One edit that replaces the span from the first edit's start to the last one's end, for
    /// the bulk path. `text` must be the text the edits were made for.
    pub fn as_single_edit(&self, text: &str) -> Option<TextEdit> {
        let first = self.edits.first()?;
        let last = self.edits.last()?;
        let spans = self.byte_spans(text);
        let changed: usize = self.edits.iter().map(|edit| edit.insert.len()).sum();
        let mut insert =
            String::with_capacity(spans[spans.len() - 1].end - spans[0].start + changed);
        let mut copied = spans[0].start;
        for (edit, span) in self.edits.iter().zip(spans.iter()) {
            insert.push_str(&text[(copied..span.start).get()]);
            insert.push_str(&edit.insert);
            copied = span.end;
        }
        Some(TextEdit::new(first.start()..last.end(), insert))
    }

    /// The whole text after the replacements.
    pub fn apply(&self, text: &str) -> String {
        let Some(edit) = self.as_single_edit(text) else {
            return text.to_owned();
        };
        let spans = self.byte_spans(text);
        let (start, end) = (spans[0].start.get(), spans[spans.len() - 1].end.get());
        let mut out = String::with_capacity(text.len() - (end - start) + edit.insert.len());
        out.push_str(&text[..start]);
        out.push_str(&edit.insert);
        out.push_str(&text[end..]);
        out
    }

    /// The edits' byte ranges in `text`, the text they were made for (Replace in Files maps
    /// them to the file's own text).
    pub fn byte_ranges(&self, text: &str) -> Vec<Range<ByteOffset>> {
        self.byte_spans(text).into_owned()
    }

    /// The edits' byte ranges in `text`: recorded while matching, or recomputed if the edits
    /// were changed or `text` is another text.
    fn byte_spans(&self, text: &str) -> Cow<'_, [Range<ByteOffset>]> {
        if self.spans.len() == self.edits.len() && self.text_len == text.len() {
            return Cow::Borrowed(&self.spans);
        }
        let mut cursor = ByteCursor::new(text);
        Cow::Owned(
            self.edits
                .iter()
                .map(|edit| cursor.byte_at(edit.start())..cursor.byte_at(edit.end()))
                .collect(),
        )
    }
}

impl Matcher {
    /// Replaces every match inside `range` (the whole text when `None`) with
    /// `template`, on a snapshot. `$`` and `$'` see the text since the previous match and up
    /// to the end of the range. All matches are found in the original text, like a global
    /// substitution, so lookbehind never sees an earlier replacement.
    pub fn replace_all(
        &self,
        text: &str,
        template: &Template,
        range: Option<Range<CharOffset>>,
        cancel: &AtomicBool,
    ) -> Result<ReplaceAll, SearchError> {
        let scope = Scope::resolve(text, range)?;
        let subject = &text[..scope.bytes.end.get()];
        let literal = template.as_literal();
        let mut matches = Matches::new(self, subject.as_bytes(), scope.bytes.start.get());
        let mut locations = self.locations();
        let names = self.capture_names();
        let mut chars = CharCursor::at(text, scope.start());
        let mut result = ReplaceAll {
            text_len: text.len(),
            ..ReplaceAll::default()
        };
        let mut previous_end = scope.bytes.start.get();
        loop {
            let found = match literal {
                Some(_) => matches.next()?,
                None => matches.next_captures(&mut locations)?,
            };
            let Some((start, end)) = found else { break };
            if cancel.load(Ordering::Relaxed) {
                return Err(SearchError::Cancelled);
            }
            result.count += 1;
            let insert = match &literal {
                Some(literal) => Cow::Borrowed(literal.as_str()),
                None => Cow::Owned(template.expand(&MatchCaptures {
                    text: subject,
                    locations: &locations,
                    names,
                    prefix: &subject[previous_end..start],
                    suffix: &subject[end..],
                })),
            };
            previous_end = end;
            if insert.as_ref() == &subject[start..end] {
                continue;
            }
            let span = ByteOffset::new(start)..ByteOffset::new(end);
            let replaced = chars.char_at(span.start)..chars.char_at(span.end);
            result
                .edits
                .push(TextEdit::new(replaced, insert.into_owned()));
            result.spans.push(span);
        }
        Ok(result)
    }

    /// The edit for Replace on one match, found earlier in the same text. Fails with
    /// [`SearchError::NotAMatch`] when the text there does not match (any more).
    pub fn replace_one(
        &self,
        text: &str,
        found: &Match,
        template: &Template,
    ) -> Result<TextEdit, SearchError> {
        let scope = Scope::resolve(text, Some(found.range()))?;
        let bytes = scope.bytes.get();
        let wanted = Some((bytes.start, bytes.end));
        let subject = text.as_bytes();
        let mut locations = self.locations();
        let mut hit = self.captures(&mut locations, subject, bytes.start, false)?;
        if hit != wanted && bytes.end > bytes.start {
            hit = self.captures(&mut locations, subject, bytes.start, true)?;
        }
        if hit != wanted {
            return Err(SearchError::NotAMatch);
        }
        let insert = match template.as_literal() {
            Some(literal) => literal,
            None => template.expand(&MatchCaptures {
                text,
                locations: &locations,
                names: self.capture_names(),
                prefix: "",
                suffix: &text[bytes.end..],
            }),
        };
        Ok(TextEdit::new(found.range(), insert))
    }
}
