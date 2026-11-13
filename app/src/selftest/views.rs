//! Self-test commands for bookmarks and the gutter, Mark and the style tokens, the split
//! view and Compare (docs/TESTING.md). Views are numbered as the user sees them: 1 is the main
//! view, 2 the second one.

use super::script::{Step, boolean};
use super::{Harness, StepResult, WAIT_TIMEOUT, equal, number};
use crate::editor::EditorPage;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use sourceview5::prelude::*;
use std::time::{Duration, Instant};
use stet_domain::diff::{LineKind, Side};
use stet_domain::marks::MarkStyle;

/// The commands and their argument counts (minimum, maximum), as in `script::COMMANDS`.
pub const COMMANDS: &[(&str, usize, usize)] = &[
    ("gutter-click", 1, 1),
    ("mark-style", 1, 1),
    ("mark-option", 2, 2),
    ("view-focus", 1, 1),
    ("drag-tab", 1, 1),
    ("drop-tab", 2, 2),
    ("scroll-to-line", 1, 1),
    ("set-split-position", 1, 1),
    ("wait-compare", 0, 0),
    ("report-compare", 0, 0),
    ("report-mark", 0, 0),
    ("assert-bookmarks", 1, 1),
    ("assert-bookmark-count", 1, 1),
    ("assert-gutter", 0, 0),
    ("assert-bookmark-steps", 1, 1),
    ("assert-marks", 2, 2),
    ("assert-marked-text", 3, 3),
    ("assert-mark-tags", 2, 2),
    ("assert-mark-stats", 1, 1),
    ("assert-line-op", 2, 2),
    ("assert-active-view", 1, 1),
    ("assert-split", 1, 1),
    ("assert-menu-at", 1, 1),
    ("click-new-tab", 1, 1),
    ("assert-new-tab-at", 1, 1),
    // Check for Updates.
    ("release-server", 1, 2),
    ("click-dialog-button", 1, 1),
    ("assert-menu-within", 0, 0),
    ("assert-menu-submenus", 0, 0),
    ("popup-submenu", 1, 1),
    ("assert-view-tabs", 2, 2),
    ("assert-view-shows", 2, 2),
    ("assert-same-document", 2, 2),
    ("assert-view-cursor", 3, 3),
    ("assert-split-position", 1, 1),
    ("assert-comparing", 1, 1),
    ("assert-compare-line", 3, 3),
    ("assert-compare-paint", 3, 3),
    ("assert-text-background", 3, 3),
    ("assert-compare-inline", 3, 3),
    ("assert-compare-pads", 2, 2),
    ("assert-compare-stats", 4, 4),
    ("assert-compare-aligned", 0, 0),
    ("assert-compare-clean", 0, 0),
    ("assert-status-lacks", 1, 1),
    ("assert-scroll-level", 0, 0),
    ("assert-compare-under", 1, 2),
    ("assert-compare-painted-under", 1, 2),
];

impl Harness {
    /// Runs one of these commands; `None` when `step` is not one.
    pub(super) async fn views_step(&self, step: &Step) -> Option<StepResult> {
        let arg = |index: usize| step.args[index].as_str();
        Some(match step.command.as_str() {
            // What GtkSourceView's gutter does for a click on a line's mark area.
            "gutter-click" => self.page().and_then(|page| {
                let line = number(arg(0))?;
                let buffer = page.buffer();
                let iter = buffer
                    .iter_at_line(line.saturating_sub(1) as i32)
                    .ok_or_else(|| format!("there is no line {line}"))?;
                let state = gtk::gdk::ModifierType::empty();
                page.view().emit_by_name::<()>(
                    "line-mark-activated",
                    &[&iter, &gtk::gdk::BUTTON_PRIMARY, &state, &1i32],
                );
                Ok(())
            }),
            "mark-style" => mark_style(arg(0)).map(|style| {
                self.window.set_mark_style(style);
            }),
            "mark-option" => boolean(arg(1)).and_then(|on| match arg(0) {
                name @ ("bookmark" | "purge") => {
                    self.window.set_find_option(name, on);
                    Ok(())
                }
                other => Err(format!("no Mark option {other}; use bookmark or purge")),
            }),
            "assert-menu-at" => match view_index(arg(0)) {
                Ok(index) => self.check_menu_at(index).await,
                Err(error) => Err(error),
            },
            "assert-menu-within" => self.check_menu_within().await,
            "click-new-tab" => match view_index(arg(0)) {
                Ok(index) => {
                    self.window.new_tab_button(index).emit_clicked();
                    self.wait_until_settled().await;
                    Ok(())
                }
                Err(error) => Err(error),
            },
            "release-server" => {
                let body = step.args.get(1).cloned().unwrap_or_default();
                release_server(arg(0), body).map(|url| self.window.set_update_url(Some(url)))
            }
            "click-dialog-button" => {
                let clicked = self
                    .window
                    .open_dialog()
                    .is_some_and(|dialog| crate::window::dialogs::click_button(&dialog, arg(0)));
                if clicked {
                    self.wait_until_settled().await;
                    Ok(())
                } else {
                    Err(format!("no open dialog has a button {}", arg(0)))
                }
            }
            "assert-new-tab-at" => match view_index(arg(0)) {
                Ok(index) => self.check_new_tab_at(index).await,
                Err(error) => Err(error),
            },
            "assert-menu-submenus" => self.check_menu_submenus().await,
            "popup-submenu" => self.popup_submenu(arg(0)).await,
            "view-focus" => match view_index(arg(0)) {
                Ok(index) => match self.window.view_page(index) {
                    Some(page) => {
                        page.view().grab_focus();
                        self.wait_until_settled().await;
                        Ok(())
                    }
                    None => Err(format!("view {} shows no tab", arg(0))),
                },
                Err(error) => Err(error),
            },
            // What libadwaita does when a tab is dropped on the other view's strip.
            "drag-tab" => match number(arg(0)) {
                Ok(index) => match self.window.tab_at(index) {
                    Some((tabs, tab_page)) => {
                        let other = if tabs == self.window.tab_view(0) {
                            1
                        } else {
                            0
                        };
                        let target = self.window.tab_view(other);
                        tabs.transfer_page(&tab_page, &target, target.n_pages());
                        self.wait_until_settled().await;
                        Ok(())
                    }
                    None => Err(format!("there is no tab {index}")),
                },
                Err(error) => Err(error),
            },
            // What happens when a tab is dropped on a strip's + or the empty part after it:
            // the strip accepts the drop, and libadwaita asks the tab's view for a new
            // window's tab view and attaches the tab there, first.
            "drop-tab" => match (number(arg(0)), view_index(arg(1))) {
                (Ok(index), Ok(view)) => match self.window.tab_at(index) {
                    Some((tabs, tab_page)) => {
                        let value = glib::BoxedValue(tab_page.to_value());
                        let accepted = self
                            .window
                            .tab_drop_target(view)
                            .emit_by_name::<bool>("drop", &[&value, &0f64, &0f64]);
                        // The signal takes the reference its handler returns, and the
                        // binding's handler returns one it doesn't own. libadwaita, emitting
                        // from C, gives it back; emitted from here, dropping the answer would
                        // take it from the window.
                        let stage = tabs
                            .emit_by_name::<Option<adw::TabView>>("create-window", &[])
                            .map(std::mem::ManuallyDrop::new);
                        match stage {
                            Some(stage) if accepted => {
                                tabs.transfer_page(&tab_page, &stage, 0);
                                self.wait_until_settled().await;
                                Ok(())
                            }
                            _ => Err(format!("the strip of view {} did not take the tab", arg(1))),
                        }
                    }
                    None => Err(format!("there is no tab {index}")),
                },
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
            "scroll-to-line" => match (self.page(), number(arg(0))) {
                (Ok(page), Ok(line)) => {
                    let view = page.view();
                    let buffer = page.buffer();
                    match buffer.iter_at_line(line.saturating_sub(1) as i32) {
                        Some(iter) => {
                            let (y, _) = view.line_yrange(&iter);
                            if let Some(adjustment) = view.vadjustment() {
                                adjustment.set_value(f64::from(y));
                            }
                            self.wait_until_settled().await;
                            Ok(())
                        }
                        None => Err(format!("there is no line {line}")),
                    }
                }
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
            "set-split-position" => number(arg(0)).map(|position| {
                self.window.set_split_position(position as i32);
            }),
            "wait-compare" => {
                let started = Instant::now();
                loop {
                    let done = self.window.comparing()
                        && !self.window.compare_busy()
                        && self.window.comparison().is_some();
                    if done {
                        self.wait_until_settled().await;
                        break Ok(());
                    }
                    if started.elapsed() > WAIT_TIMEOUT {
                        break Err("the comparison never showed".to_owned());
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
            }
            // The first frame painted after the comparison showed, from the snapshots; needs
            // `unpace-frames`, or Broadway paints once a second.
            "assert-compare-painted-under" => {
                let started = Instant::now();
                let run = loop {
                    match self.window.compare_run() {
                        Some(run) if run.painted.is_some() => break Some(run),
                        _ if started.elapsed() > WAIT_TIMEOUT => break None,
                        _ => glib::timeout_future(Duration::from_millis(2)).await,
                    }
                };
                match run {
                    Some(run) => {
                        report_compare(&run);
                        let debug = cfg!(debug_assertions) && step.args.len() > 1;
                        let painted = run.painted.unwrap_or_default();
                        number(arg(usize::from(debug))).and_then(|limit| {
                            let limit = Duration::from_millis(limit as u64);
                            if painted <= limit {
                                Ok(())
                            } else {
                                Err(format!(
                                    "the comparison was on screen after {painted:?}, over {limit:?}"
                                ))
                            }
                        })
                    }
                    None => Err("no frame was painted after the comparison".to_owned()),
                }
            }
            "report-compare" => match self.window.compare_run() {
                Some(run) => {
                    report_compare(&run);
                    Ok(())
                }
                None => Err("no comparison has run".to_owned()),
            },
            "report-mark" => match self.window.mark_stats() {
                Some(stats) => {
                    println!(
                        "  # mark: {} matches{}, {} lines bookmarked, worker {}, {:.1} ms",
                        stats.matches,
                        if stats.truncated { "+" } else { "" },
                        stats.bookmarked,
                        stats.worker,
                        stats.took.as_secs_f64() * 1e3
                    );
                    Ok(())
                }
                None => Err("nothing was marked".to_owned()),
            },
            _ => return None,
        })
    }

    /// Checks one of these assertions; `None` when `assertion` is not one.
    pub(super) fn views_check(&self, assertion: &str, args: &[String]) -> Option<StepResult> {
        let arg = |index: usize| args[index].as_str();
        Some(match assertion {
            "assert-bookmarks" => self.page().and_then(|page| {
                let lines: Vec<String> = page
                    .bookmarks()
                    .lines()
                    .iter()
                    .map(|line| (line + 1).to_string())
                    .collect();
                let shown = if lines.is_empty() {
                    "none".to_owned()
                } else {
                    lines.join(",")
                };
                equal("bookmarked lines", shown, arg(0).to_owned())
            }),
            "assert-bookmark-count" => self
                .page()
                .and_then(|page| equal("bookmarks", page.bookmarks().len(), number(arg(0))?)),
            "assert-gutter" => self.page().and_then(|page| {
                let view = page.view();
                if !view.shows_line_marks() {
                    return Err("the gutter shows no line marks".to_owned());
                }
                if page.bookmark_pixbuf().is_some() {
                    Ok(())
                } else {
                    Err("bookmarks have no picture in the gutter".to_owned())
                }
            }),
            "assert-bookmark-steps" => self.page().and_then(|page| {
                equal(
                    "bookmark history steps",
                    page.marks().history_steps(),
                    number(arg(0))?,
                )
            }),
            "assert-marks" => mark_style(arg(0))
                .and_then(|style| equal("marks", self.window.marked(style).len(), number(arg(1))?)),
            "assert-marked-text" => mark_style(arg(0)).and_then(|style| {
                let page = self.page()?;
                let index = number(arg(1))?;
                let marks = self.window.marked(style);
                let range = marks
                    .get(index.saturating_sub(1))
                    .ok_or_else(|| format!("there are {} marks", marks.len()))?;
                let buffer = page.buffer();
                let text = buffer.text(
                    &buffer.iter_at_offset(range.start as i32),
                    &buffer.iter_at_offset(range.end as i32),
                    true,
                );
                equal("marked text", text.to_string(), arg(2).to_owned())
            }),
            "assert-mark-tags" => mark_style(arg(0)).and_then(|style| {
                let page = self.page()?;
                equal(
                    "tagged marks",
                    tagged_ranges(&page, crate::editor::marks::tag_name(style)),
                    number(arg(1))?,
                )
            }),
            "assert-mark-stats" => {
                boolean(arg(0)).and_then(|worker| match self.window.mark_stats() {
                    Some(stats) => equal("marked on a worker", stats.worker, worker),
                    None => Err("nothing was marked".to_owned()),
                })
            }
            "assert-line-op" => match (
                self.window.line_op_stats(),
                boolean(arg(0)),
                boolean(arg(1)),
            ) {
                (Some(stats), Ok(worker), Ok(bulk)) => {
                    println!(
                        "  # bookmarked lines: {} edits, bulk {}, worker {}, {:.1} ms",
                        stats.edits,
                        stats.bulk,
                        stats.worker,
                        stats.took.as_secs_f64() * 1e3
                    );
                    equal("(worker, bulk)", (stats.worker, stats.bulk), (worker, bulk))
                }
                (None, _, _) => Err("no bookmarked-line operation has run".to_owned()),
                (_, Err(error), _) | (_, _, Err(error)) => Err(error),
            },
            "assert-active-view" => view_index(arg(0))
                .and_then(|index| equal("active view", self.window.active_view(), index)),
            "assert-split" => {
                boolean(arg(0)).and_then(|split| equal("split", self.window.is_split(), split))
            }
            "assert-view-tabs" => view_index(arg(0)).and_then(|index| {
                equal(
                    "tabs in the view",
                    self.window.view_pages(index).len(),
                    number(arg(1))?,
                )
            }),
            "assert-view-shows" => view_index(arg(0)).and_then(|index| {
                let name = self
                    .window
                    .view_page(index)
                    .map_or_else(|| "nothing".to_owned(), |page| page.name());
                equal("shown tab", name, arg(1).to_owned())
            }),
            "assert-same-document" => {
                let pages = self.window.pages();
                let page = |word: &str| {
                    let index = number(word)?;
                    pages
                        .get(index.saturating_sub(1))
                        .cloned()
                        .ok_or_else(|| format!("there is no tab {index}"))
                };
                match (page(arg(0)), page(arg(1))) {
                    (Ok(first), Ok(second)) => {
                        let same =
                            first.same_document(&second) && first.buffer() == second.buffer();
                        equal("one document and buffer", same, true)
                    }
                    (Err(error), _) | (_, Err(error)) => Err(error),
                }
            }
            "assert-view-cursor" => view_index(arg(0)).and_then(|index| {
                let page = self
                    .window
                    .view_page(index)
                    .ok_or_else(|| format!("view {} shows no tab", arg(0)))?;
                let (caret, _) = page.own_selection();
                let iter = page.buffer().iter_at_offset(caret as i32);
                equal(
                    "cursor (line, column)",
                    (iter.line() as usize + 1, iter.line_offset() as usize + 1),
                    (number(arg(1))?, number(arg(2))?),
                )
            }),
            "assert-split-position" => number(arg(0)).and_then(|wanted| {
                equal(
                    "divider position",
                    self.window.split_position(),
                    wanted as i32,
                )
            }),
            "assert-comparing" => boolean(arg(0))
                .and_then(|wanted| equal("comparing", self.window.comparing(), wanted)),
            "assert-compare-line" => self.compare_page(arg(0)).and_then(|(page, _, _)| {
                let line = number(arg(1))?;
                let kind = crate::editor::comparison::line_kind_at(&page, line.saturating_sub(1));
                equal("line kind", kind_name(kind).to_owned(), arg(2).to_owned())
            }),
            "assert-compare-paint" => self.compare_page(arg(0)).and_then(|(page, _, _)| {
                let line = number(arg(1))?.saturating_sub(1);
                let (painted, [red, green, blue, _]) = painted_kind(&page, line)?;
                if painted == arg(2) {
                    Ok(())
                } else {
                    Err(format!(
                        "line {} is painted {painted} (#{red:02x}{green:02x}{blue:02x}), wanted {}",
                        line + 1,
                        arg(2)
                    ))
                }
            }),
            "assert-text-background" => self.page().and_then(|page| {
                let (line, column) = (number(arg(0))?, number(arg(1))?);
                text_background(
                    &page,
                    line.saturating_sub(1),
                    column.saturating_sub(1),
                    arg(2),
                )
            }),
            "assert-compare-inline" => self.compare_page(arg(0)).and_then(|(page, _, _)| {
                let line = number(arg(1))?;
                let ranges =
                    crate::editor::comparison::inline_ranges_at(&page, line.saturating_sub(1));
                let shown = if ranges.is_empty() {
                    "none".to_owned()
                } else {
                    ranges
                        .iter()
                        .map(|range| format!("{}..{}", range.start, range.end))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                equal("changed columns", shown, arg(2).to_owned())
            }),
            "assert-compare-pads" => self.compare_page(arg(0)).and_then(|(page, _, _)| {
                equal(
                    "padding blocks",
                    page.column_view().pads().len(),
                    number(arg(1))?,
                )
            }),
            "assert-compare-stats" => match self.window.comparison() {
                Some((_, _, comparison)) => {
                    let stats = comparison.stats;
                    let wanted = (
                        number(arg(0)),
                        number(arg(1)),
                        number(arg(2)),
                        number(arg(3)),
                    );
                    match wanted {
                        (Ok(added), Ok(removed), Ok(changed), Ok(moved)) => equal(
                            "(added, removed, changed, moved)",
                            (stats.added, stats.removed, stats.changed, stats.moved),
                            (added, removed, changed, moved),
                        ),
                        (Err(error), ..)
                        | (_, Err(error), ..)
                        | (.., Err(error), _)
                        | (.., Err(error)) => Err(error),
                    }
                }
                None => Err("no comparison is shown".to_owned()),
            },
            "assert-compare-aligned" => self.check_aligned(),
            // No line of the current document is coloured or padded for a comparison.
            "assert-compare-clean" => self.page().and_then(|page| {
                let lines = page.buffer().line_count().max(1) as usize;
                let coloured = (0..lines)
                    .filter(|line| crate::editor::comparison::line_kind_at(&page, *line).is_some())
                    .count();
                let view = page.view();
                equal(
                    "(coloured lines, padding blocks, margins)",
                    (
                        coloured,
                        page.column_view().pads().len(),
                        view.top_margin() + view.bottom_margin(),
                    ),
                    (0, 0, 0),
                )
            }),
            "assert-status-lacks" => {
                let status = self.window.status_text();
                if status.contains(arg(0)) {
                    Err(format!("status {status:?} has {:?}", arg(0)))
                } else {
                    Ok(())
                }
            }
            "assert-scroll-level" => {
                let values: Vec<f64> = (0..2)
                    .filter_map(|index| self.window.view_page(index))
                    .filter_map(|page| page.view().vadjustment())
                    .map(|adjustment| adjustment.value())
                    .collect();
                match values.as_slice() {
                    [first, second] if (first - second).abs() < 1.0 => Ok(()),
                    [first, second] => {
                        Err(format!("the views are scrolled to {first} and {second}"))
                    }
                    _ => Err("the window is not split".to_owned()),
                }
            }
            "assert-compare-under" => match self.window.compare_run() {
                Some(run) => {
                    report_compare(&run);
                    // The bar is for release builds; a debug build may get its own limit.
                    let debug = cfg!(debug_assertions) && args.len() > 1;
                    number(arg(usize::from(debug))).and_then(|limit| {
                        let limit = Duration::from_millis(limit as u64);
                        if run.total <= limit {
                            Ok(())
                        } else {
                            Err(format!(
                                "the comparison took {:?}, over {limit:?}",
                                run.total
                            ))
                        }
                    })
                }
                None => Err("no comparison has run".to_owned()),
            },
            _ => return None,
        })
    }

    /// The tab that shows the `old` or `new` side of the comparison.
    fn compare_page(
        &self,
        side: &str,
    ) -> Result<(EditorPage, Side, std::rc::Rc<stet_domain::diff::Comparison>), String> {
        let (old, new, comparison) = self
            .window
            .comparison()
            .ok_or_else(|| "no comparison is shown".to_owned())?;
        match side {
            "old" => Ok((old, Side::Left, comparison)),
            "new" => Ok((new, Side::Right, comparison)),
            other => Err(format!("no side {other}; use old or new")),
        }
    }

    /// Every row of the comparison where both sides have a line puts the two lines at the
    /// same height in their views.
    fn check_aligned(&self) -> StepResult {
        let (old, _, comparison) = self.compare_page("old")?;
        let (new, _, _) = self.compare_page("new")?;
        let alignment = &comparison.alignment;
        let y = |page: &EditorPage, line: usize| {
            page.buffer()
                .iter_at_line(line as i32)
                .map(|iter| page.view().line_yrange(&iter).0)
        };
        let mut checked = 0;
        let mut worst = 0;
        for row in 0..alignment.rows() {
            let (stet_domain::diff::RowSlot::Line(left), stet_domain::diff::RowSlot::Line(right)) = (
                alignment.slot(Side::Left, row),
                alignment.slot(Side::Right, row),
            ) else {
                continue;
            };
            let (Some(left_y), Some(right_y)) = (y(&old, left), y(&new, right)) else {
                continue;
            };
            worst = worst.max((left_y - right_y).abs());
            checked += 1;
        }
        if checked == 0 {
            return Err("no row has a line on both sides".to_owned());
        }
        println!(
            "  # alignment: {checked} rows with a line on both sides, off by at most {worst} px"
        );
        if worst == 0 {
            Ok(())
        } else {
            Err(format!("rows are off by up to {worst} px"))
        }
    }
}

fn report_compare(run: &crate::window::CompareRun) {
    let ms = |duration: Duration| duration.as_secs_f64() * 1e3;
    let painted = run.painted.map_or_else(
        || "-".to_owned(),
        |painted| format!("{:.1} ms", ms(painted)),
    );
    println!(
        "  # compare: {} and {} lines, {}; snapshot {:.1} ms, diff {:.1} ms (worker), present \
         {:.1} ms, total {:.1} ms, first frame after it {painted} ({} build, run {})",
        run.lines.0,
        run.lines.1,
        run.stats.summary(),
        ms(run.snapshot),
        ms(run.diff),
        ms(run.present),
        ms(run.total),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        run.runs
    );
}

/// The number of runs of text that carry the tag `name`.
fn tagged_ranges(page: &EditorPage, name: &str) -> usize {
    let buffer = page.buffer();
    let Some(tag) = buffer.tag_table().lookup(name) else {
        return 0;
    };
    let mut iter = buffer.start_iter();
    let mut count = usize::from(iter.starts_tag(Some(&tag)));
    while iter.forward_to_tag_toggle(Some(&tag)) {
        if iter.starts_tag(Some(&tag)) {
            count += 1;
        }
    }
    count
}

fn mark_style(word: &str) -> Result<MarkStyle, String> {
    match word {
        "mark" => Ok(MarkStyle::Mark),
        number => number
            .parse::<usize>()
            .ok()
            .and_then(MarkStyle::token)
            .ok_or_else(|| format!("no style {word}; use mark or 1 to 5")),
    }
}

impl Harness {
    /// The menu is at the start of view `index`'s tab strip, left of its first tab, once
    /// the strip has been laid out.
    async fn check_menu_at(&self, index: usize) -> StepResult {
        self.wait_frames(2).await?;
        equal(
            "the view with the menu",
            self.window.menu_view(),
            Some(index),
        )?;
        let bar = self.window.tab_bar(index);
        let bar = bar.upcast_ref::<gtk::Widget>();
        let menu = self
            .window
            .menu_button()
            .compute_bounds(bar)
            .ok_or("the menu is not on its strip")?;
        let first = self.window.tab_view(index).nth_page(0);
        let (_, tab) = self
            .window
            .tab_widget(&first)
            .ok_or("the strip shows no tab")?;
        let tab = tab
            .compute_bounds(bar)
            .ok_or("the first tab is not on its strip")?;
        if menu.width() > 0.0 && menu.x() + menu.width() <= tab.x() {
            Ok(())
        } else {
            Err(format!(
                "the menu spans {:.0}–{:.0}, and the first tab starts at {:.0}",
                menu.x(),
                menu.x() + menu.width(),
                tab.x()
            ))
        }
    }

    /// View `index`'s + follows its last tab, or, once the tabs scroll, the end of the
    /// tabs on screen, and lies within the strip.
    async fn check_new_tab_at(&self, index: usize) -> StepResult {
        self.wait_frames(2).await?;
        let bar = self.window.tab_bar(index);
        let bar = bar.upcast_ref::<gtk::Widget>();
        let tabs = self.window.tab_view(index);
        let plus = self
            .window
            .new_tab_button(index)
            .compute_bounds(bar)
            .ok_or("the + is not on its strip")?;
        let last = (0..tabs.n_pages())
            .filter_map(|n| self.window.tab_widget(&tabs.nth_page(n)))
            .filter_map(|(_, tab)| tab.compute_bounds(bar))
            .map(|tab| tab.x() + tab.width())
            .fold(f32::MIN, f32::max);
        let end = self
            .window
            .tabs_end(index)
            .ok_or("the strip has no tab scroller")?;
        let after = last.min(end);
        let gap = plus.x() - after;
        if plus.width() > 0.0
            && (-1.0..=12.0).contains(&gap)
            && plus.x() + plus.width() <= bar.width() as f32
        {
            Ok(())
        } else {
            Err(format!(
                "the + spans {:.0}–{:.0} on a strip {} wide; the last tab ends at {last:.0}, \
                 and the tabs on screen at {end:.0}",
                plus.x(),
                plus.x() + plus.width(),
                bar.width()
            ))
        }
    }
}

impl Harness {
    /// The open menu lies within the window: it opens from the button's left edge rather than
    /// centred on it, half outside the window, and above its bottom edge.
    async fn check_menu_within(&self) -> StepResult {
        self.wait_frames(2).await?;
        self.menu_within(&self.open_menu()?)
    }

    /// The open menu opens its submenus beside it, as resting the pointer on their items does,
    /// and each lies within the window, one taller than the room below its item scrolling:
    /// every submenu of the first page, and the tallest of the others.
    async fn check_menu_submenus(&self) -> StepResult {
        let popover = self.open_menu()?;
        let menu = popover
            .downcast_ref::<gtk::PopoverMenu>()
            .ok_or("the menu is not a popover menu")?;
        if !menu.flags().contains(gtk::PopoverMenuFlags::NESTED) {
            return Err("the menu's submenus are not nested".to_owned());
        }
        let submenus: Vec<gtk::PopoverMenu> = crate::window::menus_in(menu.upcast_ref())
            .into_iter()
            .filter(|submenu| submenu != menu)
            .collect();
        let item_of = |submenu: &gtk::PopoverMenu| {
            submenu
                .parent()
                .and_then(|item| item.ancestor(gtk::PopoverMenu::static_type()))
        };
        let mut shown: Vec<&gtk::PopoverMenu> = submenus
            .iter()
            .filter(|submenu| item_of(submenu).as_ref() == Some(menu.upcast_ref()))
            .collect();
        if shown.len() != stet_domain::actions::Menu::ALL.len() {
            return Err(format!(
                "the menu has {} submenus, not {}",
                shown.len(),
                stet_domain::actions::Menu::ALL.len()
            ));
        }
        let tallest = submenus
            .iter()
            .filter(|submenu| !shown.contains(submenu))
            .max_by_key(|submenu| submenu.measure(gtk::Orientation::Vertical, -1).1);
        shown.extend(tallest);
        for submenu in shown {
            let mut chain = vec![submenu.clone()];
            while let Some(parent) = item_of(chain.last().unwrap_or(submenu))
                .and_then(|parent| parent.downcast::<gtk::PopoverMenu>().ok())
                .filter(|parent| parent != menu)
            {
                chain.push(parent);
            }
            for open in chain.iter().rev() {
                open.popup();
            }
            self.wait_frames(2).await?;
            let name = submenu
                .parent()
                .and_then(|item| item_text(&item))
                .unwrap_or_default();
            if !submenu.is_visible() {
                return Err(format!("the submenu {name} did not open"));
            }
            self.menu_within(submenu.upcast_ref())
                .map_err(|error| format!("its submenu {name}: {error}"))?;
            for open in &chain {
                open.popdown();
            }
            if !popover.is_visible() {
                popover.popup();
            }
            self.wait_frames(1).await?;
        }
        Ok(())
    }

    /// Opens the open menu's submenu `name`, as resting the pointer on its item does.
    async fn popup_submenu(&self, name: &str) -> StepResult {
        let popover = self.open_menu()?;
        let submenu = crate::window::menus_in(popover.upcast_ref())
            .into_iter()
            .find(|submenu| {
                submenu
                    .parent()
                    .and_then(|item| item_text(&item))
                    .as_deref()
                    == Some(name)
            })
            .ok_or_else(|| format!("the menu has no submenu {name}"))?;
        submenu.popup();
        self.wait_frames(2).await
    }

    fn open_menu(&self) -> Result<gtk::Popover, String> {
        self.window
            .menu_button()
            .popover()
            .filter(WidgetExt::is_visible)
            .ok_or_else(|| "the menu is not open".to_owned())
    }

    fn menu_within(&self, popover: &gtk::Popover) -> StepResult {
        // A submenu's popup is placed relative to its parent menu's, that one's to the window.
        let (mut popup_x, mut popup_y, mut popups) = (0, 0, 0);
        let mut surface = popover.surface();
        while let Some(popup) =
            surface.and_then(|surface| surface.downcast::<gtk::gdk::Popup>().ok())
        {
            popup_x += popup.position_x();
            popup_y += popup.position_y();
            popups += 1;
            surface = popup.parent();
        }
        if popups == 0 {
            return Err("the menu has no popup surface".to_owned());
        }
        let window = self.window.window();
        let (window_x, window_y) = window.surface_transform();
        let (inner_x, inner_y) = popover.surface_transform();
        let left = f64::from(popup_x) + inner_x - window_x;
        let right = left + f64::from(popover.width());
        let top = f64::from(popup_y) + inner_y - window_y;
        let bottom = top + f64::from(popover.height());
        let (width, height) = (f64::from(window.width()), f64::from(window.height()));
        if left >= 0.0 && right <= width && top >= 0.0 && bottom <= height {
            Ok(())
        } else {
            Err(format!(
                "the menu spans {left:.0}–{right:.0} × {top:.0}–{bottom:.0} in a window \
                 {width:.0} × {height:.0}"
            ))
        }
    }
}

/// A server on this computer that answers Check for Updates' one request in GitHub's place,
/// with HTTP `status` and `body`; `none`: a port where nothing listens. Returns its URL.
fn release_server(status: &str, body: String) -> Result<String, String> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let url = format!("http://{address}/repos/PixDevsApps/Stet/releases/latest");
    if status == "none" {
        return Ok(url);
    }
    let status: u16 = status
        .parse()
        .map_err(|_| format!("no HTTP status {status}; use a number or none"))?;
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = Vec::new();
        let mut buffer = [0; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        let answer = format!(
            "HTTP/1.1 {status} Answer\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(answer.as_bytes());
    });
    Ok(url)
}

/// A menu item's text: GtkModelButton's `text`.
fn item_text(item: &gtk::Widget) -> Option<String> {
    item.find_property("text")
        .and_then(|_| item.property::<Option<String>>("text"))
}

fn view_index(word: &str) -> Result<usize, String> {
    match word {
        "1" => Ok(0),
        "2" => Ok(1),
        other => Err(format!("no view {other}; use 1 or 2")),
    }
}

/// The comparison background `page`'s view paints at the right end of `line` (zero-based), as
/// rendered: the kind whose scheme colour the pixel has, or "none"; and the pixel.
fn painted_kind(page: &EditorPage, line: usize) -> Result<(&'static str, [u8; 4]), String> {
    let view = page.view();
    let buffer = page.buffer();
    let iter = buffer
        .iter_at_line(line as i32)
        .ok_or_else(|| format!("there is no line {}", line + 1))?;
    let (top, _) = view.line_yrange(&iter);
    let (_, y) = view.buffer_to_window_coords(gtk::TextWindowType::Widget, 0, top);
    let pixel = super::screenshot::pixel(view.upcast_ref(), view.width() - 4, y + 2)?;
    let scheme = buffer
        .style_scheme()
        .ok_or_else(|| "the buffer has no style scheme".to_owned())?;
    let kinds = [
        LineKind::Added,
        LineKind::Removed,
        LineKind::Changed,
        LineKind::Moved,
    ];
    let painted = kinds.into_iter().find(|&kind| {
        scheme
            .style(crate::editor::comparison::style_name(kind))
            .and_then(|style| style.background())
            .and_then(|value| gtk::gdk::RGBA::parse(value.as_str()).ok())
            .is_some_and(|color| near(&color, pixel))
    });
    Ok((kind_name(painted), pixel))
}

/// Whether `pixel` (red, green, blue and alpha bytes) is `color`, give or take rounding.
fn near(color: &gtk::gdk::RGBA, pixel: [u8; 4]) -> bool {
    [color.red(), color.green(), color.blue()]
        .iter()
        .zip(pixel)
        .all(|(wanted, got)| ((wanted * 255.0).round() - f32::from(got)).abs() <= 2.0)
}

/// Checks that `page`'s view paints the top left corner of the character at `line` and
/// `column` (zero-based), above any glyph, in the background of the scheme's `style`, as
/// rendered.
fn text_background(page: &EditorPage, line: usize, column: usize, style: &str) -> StepResult {
    let view = page.view();
    let buffer = page.buffer();
    let iter = buffer
        .iter_at_line_offset(line as i32, column as i32)
        .ok_or_else(|| format!("there is no line {}, column {}", line + 1, column + 1))?;
    let cell = view.iter_location(&iter);
    let (x, y) =
        view.buffer_to_window_coords(gtk::TextWindowType::Widget, cell.x() + 1, cell.y() + 1);
    let [red, green, blue, _] = super::screenshot::pixel(view.upcast_ref(), x, y)?;
    let wanted = buffer
        .style_scheme()
        .and_then(|scheme| scheme.style(style))
        .and_then(|style| style.background())
        .and_then(|value| gtk::gdk::RGBA::parse(value.as_str()).ok())
        .ok_or_else(|| format!("the scheme has no background for {style}"))?;
    if near(&wanted, [red, green, blue, 255]) {
        Ok(())
    } else {
        Err(format!(
            "line {}, column {} is painted #{red:02x}{green:02x}{blue:02x}, wanted {style} ({})",
            line + 1,
            column + 1,
            wanted.to_str()
        ))
    }
}

fn kind_name(kind: Option<LineKind>) -> &'static str {
    match kind {
        Some(LineKind::Added) => "added",
        Some(LineKind::Removed) => "removed",
        Some(LineKind::Changed) => "changed",
        Some(LineKind::Moved) => "moved",
        None => "none",
    }
}
