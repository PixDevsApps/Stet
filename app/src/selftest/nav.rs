//! Self-test commands for M8: quick open, the Ctrl+Tab switcher (Ctrl held and released
//! through the window's key controller), back and forward (and the mouse's buttons), pinned
//! tabs and the tab menu, first-line names, and Replace in Files.

use super::script::{Step, boolean};
use super::{Harness, StepResult, WAIT_TIMEOUT, equal, number};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::IntoGlib;
use gtk4::prelude::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The commands this module runs as steps; the assertions are in [`Harness::nav_check`].
const STEPS: &[&str] = &[
    "quick-open",
    "quick-open-in",
    "quick-open-type",
    "quick-open-select",
    "quick-open-activate",
    "quick-open-close",
    "wait-quick-open",
    "report-quick-open",
    "hold-ctrl",
    "release-ctrl",
    "switcher-key",
    "mouse-button",
    "wait-rif",
    "report-rif",
];

fn ms(duration: Option<Duration>) -> String {
    duration.map_or_else(
        || "-".to_owned(),
        |duration| format!("{:.1} ms", duration.as_secs_f64() * 1e3),
    )
}

impl Harness {
    /// Runs an M8 command; `None` when `step` is not one.
    pub(super) async fn nav_step(&self, step: &Step) -> Option<StepResult> {
        if !STEPS.contains(&step.command.as_str()) {
            return None;
        }
        Some(self.run_nav_step(step).await)
    }

    async fn run_nav_step(&self, step: &Step) -> StepResult {
        let args = &step.args;
        let arg = |index: usize| args[index].as_str();
        let quick_open = &self.window.inner.nav.quick_open;
        match step.command.as_str() {
            // Opens quick open, as Ctrl+P does, and waits until it is on screen; quick-open-in
            // lists the project of the folder given instead of the current document's.
            "quick-open" | "quick-open-in" => {
                if step.command == "quick-open" {
                    self.window
                        .open_quick_open(args.first().map_or("", String::as_str));
                } else {
                    let folder = match arg(0).strip_prefix("~/") {
                        Some(rest) => std::env::home_dir().ok_or("no home folder")?.join(rest),
                        None => PathBuf::from(arg(0)),
                    };
                    self.window
                        .open_quick_open_from(args.get(1).map_or("", String::as_str), Some(folder));
                }
                let started = Instant::now();
                while !quick_open.popover.is_visible() {
                    if started.elapsed() > WAIT_TIMEOUT {
                        return Err("quick open never opened".to_owned());
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                Ok(())
            }
            "quick-open-type" => {
                quick_open.entry.set_text(arg(0));
                Ok(())
            }
            "quick-open-select" => {
                let index = number(arg(0))?;
                quick_open.select_row(index)
            }
            "quick-open-activate" => {
                quick_open.activate_selected();
                self.wait_idle().await
            }
            // What Escape does in the field.
            "quick-open-close" => {
                quick_open.entry.emit_by_name::<()>("stop-search", &[]);
                Ok(())
            }
            "wait-quick-open" => {
                let limit = args
                    .first()
                    .map(|seconds| number(seconds).map(|seconds| seconds as u64))
                    .transpose()?
                    .map_or(WAIT_TIMEOUT, Duration::from_secs);
                let started = Instant::now();
                while !quick_open.is_settled() {
                    if started.elapsed() > limit {
                        return Err(format!("quick open did not settle in {limit:?}"));
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                Ok(())
            }
            "report-quick-open" => {
                let stats = quick_open.stats();
                let walk = stats.walk.unwrap_or_default();
                println!(
                    "  # quick open: first rows {}, first project rows {}, walk {} (first \
                     batch {}), {} files{}, slowest ranking {}, {} rankings shown, last query \
                     to its rows {}",
                    ms(stats.first_rows),
                    ms(stats.first_project_rows),
                    ms(Some(walk.elapsed)),
                    ms(walk.first_batch),
                    stats.files,
                    if walk.truncated { " (capped)" } else { "" },
                    ms(Some(stats.slowest_rank)),
                    stats.rankings,
                    ms(stats.last_rows)
                );
                Ok(())
            }
            "hold-ctrl" => {
                self.emit_window_key("key-pressed", "Control_L");
                Ok(())
            }
            "release-ctrl" => {
                self.emit_window_key("key-released", "Control_L");
                self.wait_until_settled().await;
                Ok(())
            }
            "switcher-key" => {
                self.emit_window_key("key-pressed", arg(0));
                self.wait_until_settled().await;
                Ok(())
            }
            // What the mouse's back (8) and forward (9) buttons do.
            "mouse-button" => match arg(0) {
                "8" => {
                    self.window.mouse_navigation(true);
                    self.wait_idle().await
                }
                "9" => {
                    self.window.mouse_navigation(false);
                    self.wait_idle().await
                }
                other => Err(format!("button {other}: use 8 (back) or 9 (forward)")),
            },
            // What a right click on tab N does before its menu opens, or `none` as it closes.
            "wait-rif" => {
                let started = Instant::now();
                while self.window.replace_in_files_running() {
                    if started.elapsed() > super::IDLE_TIMEOUT {
                        return Err("Replace in Files did not finish".to_owned());
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                self.wait_idle().await
            }
            "report-rif" => {
                let stats = self
                    .window
                    .replace_in_files_stats()
                    .ok_or("Replace in Files has not run")?;
                println!(
                    "  # replace in files: {} replacements in {} files ({} open), {} \
                     skipped, {} failed, {} files searched, {} on the worker, {} in all{}",
                    stats.replacements,
                    stats.files,
                    stats.open_documents,
                    stats.skipped,
                    stats.failed,
                    stats.summary.files_searched,
                    ms(Some(stats.summary.elapsed)),
                    ms(Some(stats.total)),
                    if stats.summary.cancelled {
                        format!(", stopped after {}", ms(stats.stopped_after))
                    } else {
                        String::new()
                    }
                );
                Ok(())
            }
            other => Err(format!("unknown command {other}")),
        }
    }

    /// Emits a key event on the window's switcher controller, with Ctrl held for every key but
    /// Ctrl's own press.
    fn emit_window_key(&self, signal: &str, key: &str) {
        let keyval = gtk::gdk::Key::from_name(key).unwrap_or(gtk::gdk::Key::VoidSymbol);
        let state = if signal == "key-released" || key != "Control_L" {
            gtk::gdk::ModifierType::CONTROL_MASK
        } else {
            gtk::gdk::ModifierType::empty()
        };
        let values: [&dyn glib::value::ToValue; 3] = [&keyval.into_glib(), &0u32, &state];
        let keys = &self.window.inner.nav.keys;
        if signal == "key-pressed" {
            keys.emit_by_name::<bool>(signal, &values);
        } else {
            keys.emit_by_name::<()>(signal, &values);
        }
    }

    /// Runs an M8 assertion; `None` when `assertion` is not one.
    pub(super) fn nav_check(&self, assertion: &str, args: &[String]) -> Option<StepResult> {
        let arg = |index: usize| args[index].as_str();
        let quick_open = &self.window.inner.nav.quick_open;
        Some(match assertion {
            "assert-quick-open-row" => number(arg(0)).and_then(|index| {
                let labels = quick_open.shown_labels();
                equal(
                    &format!("quick open row {index}"),
                    labels.get(index).cloned().unwrap_or_default(),
                    arg(1).to_owned(),
                )
            }),
            "assert-quick-open-contains" | "assert-quick-open-lacks" => {
                let labels = quick_open.shown_labels();
                let found = labels.iter().any(|label| label == arg(0));
                let wanted = assertion == "assert-quick-open-contains";
                if found == wanted {
                    Ok(())
                } else {
                    Err(format!("quick open shows {labels:?}"))
                }
            }
            "assert-quick-open-count" => number(arg(0))
                .and_then(|wanted| equal("rows", quick_open.shown_labels().len(), wanted)),
            "assert-quick-open-positions" => number(arg(0)).and_then(|index| {
                let positions = quick_open
                    .shown_positions(index)
                    .ok_or_else(|| format!("there is no row {index}"))?;
                let shown = positions
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                equal("matched characters", shown, arg(1).to_owned())
            }),
            "assert-quick-open-files" => number(arg(0))
                .and_then(|wanted| equal("project files", quick_open.stats().files, wanted)),
            "assert-quick-open-note" => {
                let note = quick_open.note.label();
                if note.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("the note {note:?} lacks {:?}", arg(0)))
                }
            }
            "assert-quick-open-shown" => boolean(arg(0)).and_then(|wanted| {
                equal("quick open shown", quick_open.popover.is_visible(), wanted)
            }),
            "assert-switcher" => boolean(arg(0)).and_then(|wanted| {
                let switcher = &self.window.inner.nav.switcher;
                equal("switcher shown", switcher.is_shown(), wanted)?;
                if let Some(selected) = args.get(1) {
                    let (_, current) = switcher.shown();
                    equal("selected", current.unwrap_or_default(), selected.clone())?;
                }
                Ok(())
            }),
            "assert-switching" => boolean(arg(0))
                .and_then(|wanted| equal("switching", self.window.switching(), wanted)),
            "assert-recent-order" => equal(
                "recently used order",
                self.window.recent_order().join("|"),
                arg(0).to_owned(),
            ),
            "assert-places" => {
                let (places, current) = self.window.history_places();
                let shown = places
                    .iter()
                    .map(|(name, line)| format!("{name}:{line}"))
                    .collect::<Vec<_>>()
                    .join("|");
                let current = current.map_or_else(|| "-".to_owned(), |index| index.to_string());
                equal(
                    "places (current)",
                    format!("{shown} ({current})"),
                    format!("{} ({})", arg(0), arg(1)),
                )
            }
            "assert-pinned" => number(arg(0)).and_then(|index| {
                let pages = self.window.pages();
                let page = pages
                    .get(index.saturating_sub(1))
                    .ok_or_else(|| format!("there is no tab {index}"))?;
                equal("pinned", self.window.is_pinned(page), boolean(arg(1))?)
            }),
            "assert-tab-icon" => number(arg(0)).and_then(|index| {
                let (_, tab) = self
                    .window
                    .tab_at(index)
                    .ok_or_else(|| format!("there is no tab {index}"))?;
                let icon = tab
                    .icon()
                    .map_or_else(|| "none".to_owned(), |icon| icon_name(&icon));
                equal("tab icon", icon, arg(1).to_owned())
            }),
            "assert-tab-menu-shown" => boolean(arg(1)).and_then(|wanted| {
                let items = crate::window::tab_menu_items(&self.window.inner.views[0].tabs);
                let shown = items.iter().any(|(label, action)| {
                    label == arg(0)
                        && stet_domain::actions::ActionId::from_name(action)
                            .and_then(|id| self.window.action(id))
                            .is_some_and(|action| action.is_enabled())
                });
                equal(&format!("tab menu shows {}", arg(0)), shown, wanted)
            }),
            "assert-save-name" => self
                .page()
                .map(|page| self.window.save_as_name(&page))
                .and_then(|name| equal("Save As name", name, arg(0).to_owned())),
            "assert-rif" => {
                let stats = self
                    .window
                    .replace_in_files_stats()
                    .ok_or_else(|| "Replace in Files has not run".to_owned());
                stats.and_then(|stats| {
                    let value = match arg(0) {
                        "replacements" => stats.replacements.to_string(),
                        "files" => stats.files.to_string(),
                        "skipped" => stats.skipped.to_string(),
                        "failed" => stats.failed.to_string(),
                        "open" => stats.open_documents.to_string(),
                        "searched" => stats.summary.files_searched.to_string(),
                        "cancelled" => stats.summary.cancelled.to_string(),
                        other => {
                            return Err(format!(
                                "no field {other}; use replacements, files, skipped, failed, \
                                 open, searched or cancelled"
                            ));
                        }
                    };
                    equal(arg(0), value, arg(1).to_owned())
                })
            }
            _ => return None,
        })
    }
}

/// A themed icon's first name.
fn icon_name(icon: &gtk::gio::Icon) -> String {
    icon.downcast_ref::<gtk::gio::ThemedIcon>()
        .and_then(|themed| themed.names().first().map(|name| name.to_string()))
        .unwrap_or_else(|| "other".to_owned())
}
