//! A comparison as one document shows it (M7): a background on added, removed, changed and
//! moved lines (`paragraph-background`, the whole width of the line), a stronger one on the
//! characters that differ inside changed lines, and padding wherever the other side has lines
//! this one lacks, so that equal lines face each other in the two views: `pixels-below-lines`
//! on the line before the gap, or the view's top or bottom margin at either end, which the view
//! paints over in the padding colour ([`Pad`]). GTK paints no paragraph background on an empty
//! line, so the view paints those lines' backgrounds itself. The colours are the theme's
//! (scheme styles).

use crate::column::Pad;
use crate::editor::EditorPage;
use gtk4 as gtk;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use stet_domain::diff::{Comparison, LineKind, Side};
use stet_domain::theme::{
    COMPARE_ADDED_STYLE, COMPARE_CHANGED_STYLE, COMPARE_INLINE_STYLE, COMPARE_MOVED_STYLE,
    COMPARE_REMOVED_STYLE,
};

/// The line tags, by kind, and the scheme styles they take their colour from.
const LINE_TAGS: [(LineKind, &str, &str); 4] = [
    (LineKind::Added, "stet-compare-added", COMPARE_ADDED_STYLE),
    (
        LineKind::Removed,
        "stet-compare-removed",
        COMPARE_REMOVED_STYLE,
    ),
    (
        LineKind::Changed,
        "stet-compare-changed",
        COMPARE_CHANGED_STYLE,
    ),
    (LineKind::Moved, "stet-compare-moved", COMPARE_MOVED_STYLE),
];
const INLINE_TAG: &str = "stet-compare-changed-text";
/// Padding tags are named by their height: `stet-compare-pad-36`.
const PAD_TAG_PREFIX: &str = "stet-compare-pad-";

/// What one side's presentation took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Presented {
    pub line_runs: usize,
    /// Empty lines among them, which the view paints.
    pub empty_lines: usize,
    pub inline_ranges: usize,
    pub pads: usize,
    /// The height of a line it padded with, in pixels.
    pub line_height: i32,
}

fn tag_name(kind: LineKind) -> &'static str {
    LINE_TAGS
        .iter()
        .find(|(each, _, _)| *each == kind)
        .map_or("stet-compare-changed", |(_, tag, _)| tag)
}

/// The scheme style a line of `kind` takes its background from.
pub fn style_name(kind: LineKind) -> &'static str {
    LINE_TAGS
        .iter()
        .find(|(each, _, _)| *each == kind)
        .map_or(COMPARE_CHANGED_STYLE, |(_, _, style)| style)
}

fn scheme_background(scheme: &sourceview5::StyleScheme, style: &str) -> Option<gtk::gdk::RGBA> {
    let value = scheme.style(style)?.background()?;
    gtk::gdk::RGBA::parse(value.as_str()).ok()
}

/// Gives the comparison's tags the colours of `scheme` (a theme change).
pub fn recolor(buffer: &sourceview5::Buffer, scheme: &sourceview5::StyleScheme) {
    let table = buffer.tag_table();
    for (_, name, style) in LINE_TAGS {
        if let Some(tag) = table.lookup(name) {
            tag.set_paragraph_background_rgba(scheme_background(scheme, style).as_ref());
        }
    }
    if let Some(tag) = table.lookup(INLINE_TAG) {
        tag.set_background_rgba(scheme_background(scheme, COMPARE_INLINE_STYLE).as_ref());
    }
}

/// The tag `name`, made the first time with the scheme's colour.
fn tag(buffer: &sourceview5::Buffer, name: &str) -> gtk::TextTag {
    if let Some(tag) = buffer.tag_table().lookup(name) {
        return tag;
    }
    let tag = buffer
        .create_tag(Some(name), &[])
        .expect("a tag name that is not taken");
    if let Some(scheme) = buffer.style_scheme() {
        recolor(buffer, &scheme);
    }
    tag
}

/// The height of one line of text in `view`, without any padding, in pixels.
pub fn line_height(view: &sourceview5::View) -> i32 {
    let measured = view.iter_location(&view.buffer().start_iter()).height();
    if measured > 0 {
        return measured;
    }
    let metrics = view.pango_context().metrics(None, None);
    ((metrics.ascent() + metrics.descent()) as f64 / gtk::pango::SCALE as f64).ceil() as i32
}

/// Removes the comparison from `page`'s document and every view of it.
pub fn clear(page: &EditorPage) {
    let buffer = page.buffer();
    let (start, end) = (buffer.start_iter(), buffer.end_iter());
    let table = buffer.tag_table();
    let mut names: Vec<String> = LINE_TAGS
        .iter()
        .map(|(_, tag, _)| tag.to_string())
        .collect();
    names.push(INLINE_TAG.to_owned());
    table.foreach(|tag| {
        if let Some(name) = tag.name()
            && name.starts_with(PAD_TAG_PREFIX)
        {
            names.push(name.to_string());
        }
    });
    for name in names {
        if table.lookup(&name).is_some() {
            buffer.remove_tag_by_name(&name, &start, &end);
        }
    }
    for view_page in page.document_pages() {
        let view = view_page.view();
        view.set_top_margin(0);
        view.set_bottom_margin(0);
        view_page.column_view().set_pads(Vec::new());
        view_page.column_view().set_empty_lines(Vec::new());
    }
}

/// Shows `side` of `comparison` in `page`'s document.
pub fn present(page: &EditorPage, comparison: &Comparison, side: Side) -> Presented {
    clear(page);
    let buffer = page.buffer();
    let line_start = |line: usize| {
        buffer
            .iter_at_line(line as i32)
            .unwrap_or_else(|| buffer.end_iter())
    };
    let mut presented = Presented::default();
    let mut empty_lines = Vec::new();
    for run in comparison.line_runs(side) {
        let tag = tag(&buffer, tag_name(run.kind));
        let start = line_start(run.lines.start);
        buffer.apply_tag(&tag, &start, &line_start(run.lines.end));
        presented.line_runs += 1;
        let mut iter = start;
        for line in run.lines.clone() {
            if iter.line().max(0) as usize != line {
                break;
            }
            if iter.ends_line() {
                empty_lines.push((line, style_name(run.kind)));
            }
            iter.forward_line();
        }
    }
    presented.empty_lines = empty_lines.len();
    let inline = tag(&buffer, INLINE_TAG);
    for diff in &comparison.inline {
        let line = diff.line(side) as i32;
        for columns in diff.columns(side) {
            let (Some(from), Some(to)) = (
                buffer.iter_at_line_offset(line, columns.start as i32),
                buffer.iter_at_line_offset(line, columns.end as i32),
            ) else {
                continue;
            };
            buffer.apply_tag(&inline, &from, &to);
            presented.inline_ranges += 1;
        }
    }
    let height = line_height(page.view());
    presented.line_height = height;
    let line_count = buffer.line_count().max(1) as usize;
    let (mut top, mut bottom) = (0, 0);
    let mut paints = Vec::new();
    for pad in comparison.alignment.pads(side) {
        let pixels = pad.rows as i32 * height;
        if pad.before == 0 {
            top += pixels;
            paints.push(Pad::Above(pixels));
        } else if pad.before >= line_count {
            bottom += pixels;
            paints.push(Pad::End(pixels));
        } else {
            let line = pad.before - 1;
            let name = format!("{PAD_TAG_PREFIX}{pixels}");
            let tag = buffer.tag_table().lookup(&name).unwrap_or_else(|| {
                buffer
                    .create_tag(Some(&name), &[("pixels-below-lines", &pixels)])
                    .expect("a tag name that is not taken")
            });
            let start = line_start(line);
            let mut end = start;
            end.forward_char();
            buffer.apply_tag(&tag, &start, &end);
            paints.push(Pad::Below { line, pixels });
        }
        presented.pads += 1;
    }
    for view_page in page.document_pages() {
        let view = view_page.view();
        view.set_top_margin(top);
        view.set_bottom_margin(bottom);
        view_page.column_view().set_pads(paints.clone());
        view_page.column_view().set_empty_lines(empty_lines.clone());
    }
    presented
}

/// The kind of difference on `line` of `page`'s document, from its tags, for the self-test.
pub fn line_kind_at(page: &EditorPage, line: usize) -> Option<LineKind> {
    let buffer = page.buffer();
    let iter = buffer.iter_at_line(line as i32)?;
    LINE_TAGS.iter().find_map(|(kind, name, _)| {
        let tag = buffer.tag_table().lookup(name)?;
        iter.has_tag(&tag).then_some(*kind)
    })
}

/// The character ranges of `line` that the changed-text tag covers, for the self-test.
pub fn inline_ranges_at(page: &EditorPage, line: usize) -> Vec<std::ops::Range<usize>> {
    let buffer = page.buffer();
    let Some(tag) = buffer.tag_table().lookup(INLINE_TAG) else {
        return Vec::new();
    };
    let Some(mut iter) = buffer.iter_at_line(line as i32) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    let mut start = None;
    loop {
        let column = iter.line_offset().max(0) as usize;
        match (iter.has_tag(&tag), start) {
            (true, None) => start = Some(column),
            (false, Some(from)) => {
                ranges.push(from..column);
                start = None;
            }
            _ => {}
        }
        if iter.ends_line() {
            if let Some(from) = start {
                ranges.push(from..column);
            }
            break;
        }
        iter.forward_char();
    }
    ranges
}
