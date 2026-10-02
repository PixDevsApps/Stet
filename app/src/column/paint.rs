//! Painting the rectangle in the view's below-text layer, which is in buffer coordinates: the
//! theme's selection colour with some transparency over the block, and a thin bar in the
//! caret colour on every line of a column caret (on the cursor line of a block). Only the
//! visible lines are painted. Each edge is at its character's x plus whole cells, the exact
//! tab stops of ADR-015 make cells inside tabs and past the end of a line land on the grid,
//! and wide characters push the edges right as their glyphs do.

use super::view::{Pad, StetView};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, graphene};
use sourceview5::prelude::*;
use stet_domain::column::{LineSpan, Rect, VisualPos, locate};

/// How opaque the block's fill is.
const FILL_ALPHA: f32 = 0.85;
const CARET_WIDTH: f32 = 2.0;

/// Paints the visible rows of `rect`; returns how many.
pub(super) fn paint(view: &StetView, rect: Rect, snapshot: &gtk::Snapshot) -> usize {
    let buffer = view.buffer();
    let visible = view.visible_rect();
    let top = view.line_at_y(visible.y()).0.line().max(0) as usize;
    let bottom = view
        .line_at_y(visible.y() + visible.height())
        .0
        .line()
        .max(0) as usize;
    let (first, last) = (rect.top().max(top), rect.bottom().min(bottom));
    if first > last {
        return 0;
    }
    let (fill, caret) = colors(view);
    let advance = view.advance() as f32;
    let tab = view.tab();
    let mut rows = 0;
    for line in first..=last {
        let Some(start) = buffer.iter_at_line(line as i32) else {
            break;
        };
        let text = view.line_text(line);
        let span = LineSpan::new(line, &text, start.offset().max(0) as usize, rect, tab);
        let left = view.iter_location(&buffer.iter_at_offset(span.chars.start as i32));
        let x = left.x() as f32 + span.pad_left as f32 * advance;
        let (y, height) = (left.y() as f32, left.height() as f32);
        if rect.is_empty() {
            snapshot.append_color(&caret, &graphene::Rect::new(x, y, CARET_WIDTH, height));
        } else {
            let right = view.iter_location(&buffer.iter_at_offset(span.chars.end as i32));
            let width = right.x() as f32 + span.pad_right as f32 * advance - x;
            snapshot.append_color(&fill, &graphene::Rect::new(x, y, width, height));
            if line == rect.cursor.line {
                let at = if rect.cursor.column == rect.left() {
                    x
                } else {
                    x + width
                };
                snapshot.append_color(&caret, &graphene::Rect::new(at, y, CARET_WIDTH, height));
            }
        }
        rows += 1;
    }
    rows
}

/// Paints the padding a comparison put between lines in the scheme's padding colour (M7), so
/// it reads as no text rather than as an empty line.
pub(super) fn pads(view: &StetView, pads: &[Pad], snapshot: &gtk::Snapshot) {
    let buffer = view.buffer();
    let visible = view.visible_rect();
    let color = scheme_color(view, stet_domain::theme::COMPARE_PAD_STYLE)
        .unwrap_or_else(|| view.color().with_alpha(0.08));
    let (x, width) = band(view, &buffer.start_iter());
    let (top, bottom) = (visible.y(), visible.y() + visible.height());
    for pad in pads {
        let (y, height) = match *pad {
            Pad::Above(pixels) => (-pixels, pixels),
            Pad::Below { line, pixels } => {
                let Some(iter) = buffer.iter_at_line(line as i32) else {
                    continue;
                };
                let (y, height) = view.line_yrange(&iter);
                (y + height - pixels, pixels)
            }
            Pad::End(pixels) => {
                let (y, height) = view.line_yrange(&buffer.end_iter());
                (y + height, pixels)
            }
        };
        if y + height < top || y > bottom {
            continue;
        }
        snapshot.append_color(
            &color,
            &graphene::Rect::new(x, y as f32, width, height as f32),
        );
    }
}

/// Paints the background a comparison gives the visible ones of `lines` (empty lines, in
/// order, with their scheme styles), as GTK paints a paragraph background (M7): GTK skips a
/// line without characters.
pub(super) fn empty_lines(
    view: &StetView,
    lines: &[(usize, &'static str)],
    snapshot: &gtk::Snapshot,
) {
    let buffer = view.buffer();
    let visible = view.visible_rect();
    let top = view.line_at_y(visible.y()).0.line().max(0) as usize;
    let bottom = view
        .line_at_y(visible.y() + visible.height())
        .0
        .line()
        .max(0) as usize;
    let mut colors: Vec<(&str, Option<gdk::RGBA>)> = Vec::new();
    let first = lines.partition_point(|(line, _)| *line < top);
    for &(line, style) in lines[first..]
        .iter()
        .take_while(|(line, _)| *line <= bottom)
    {
        let color = match colors.iter().find(|(each, _)| *each == style) {
            Some((_, color)) => *color,
            None => {
                let color = scheme_color(view, style);
                colors.push((style, color));
                color
            }
        };
        let (Some(color), Some(iter)) = (color, buffer.iter_at_line(line as i32)) else {
            continue;
        };
        let (y, height) = view.line_yrange(&iter);
        let (x, width) = band(view, &iter);
        snapshot.append_color(
            &color,
            &graphene::Rect::new(x, y as f32, width, height as f32),
        );
    }
}

/// Where the background of `iter`'s line starts and how wide it is, as GTK paints a paragraph
/// background: from the line's left margin to the right edge of the visible text.
fn band(view: &StetView, iter: &gtk::TextIter) -> (f32, f32) {
    let visible = view.visible_rect();
    let left = view.iter_location(iter).x();
    (left as f32, (visible.x() + visible.width() - left) as f32)
}

/// A scheme style's background, as the theme sets it.
fn scheme_color(view: &StetView, name: &str) -> Option<gdk::RGBA> {
    let scheme = view.source_buffer().style_scheme()?;
    let value = scheme.style(name)?.background()?;
    gdk::RGBA::parse(value.as_str()).ok()
}

/// The x of `pos` in buffer coordinates.
pub(super) fn x_of(view: &StetView, pos: VisualPos) -> f64 {
    let iter = view.iter_at_pos(pos);
    let at = locate(
        &view.line_text(iter.line().max(0) as usize),
        pos.column,
        view.tab(),
    );
    f64::from(view.iter_location(&iter).x()) + at.offset_cells() as f64 * view.advance()
}

/// The fill and caret colours from the buffer's style scheme (the Omarchy theme), recoloured
/// whenever the scheme changes since every frame reads them.
fn colors(view: &StetView) -> (gdk::RGBA, gdk::RGBA) {
    let text = view.color();
    let scheme = view.source_buffer().style_scheme();
    let style = |name: &str, background: bool| {
        let style = scheme.as_ref()?.style(name)?;
        let value = if background {
            style.background()
        } else {
            style.foreground()
        }?;
        gdk::RGBA::parse(value.as_str()).ok()
    };
    let fill = style("selection", true).map_or_else(
        || text.with_alpha(0.25),
        |color| color.with_alpha(FILL_ALPHA),
    );
    let caret = style("cursor", false)
        .or_else(|| style("text", false))
        .unwrap_or(text);
    (fill, caret)
}
