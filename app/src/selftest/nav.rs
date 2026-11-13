//! Self-test commands for quick open, the Ctrl+Tab switcher (Ctrl held and released
//! through the window's key controller), back and forward (and the mouse's buttons), pinned
//! tabs and the tab menu, first-line names, and Replace in Files.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, WAIT_TIMEOUT, equal, number};
use crate::window::Window;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::IntoGlib;
use gtk4::prelude::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) const COMMANDS: &[Command] = commands![
    "quick-open" (0, 1) => async quick_open,
    "quick-open-in" (1, 2) => async quick_open_in,
    "quick-open-type" (1, 1) => quick_open_type,
    "quick-open-select" (1, 1) => quick_open_select,
    "quick-open-activate" (0, 0) => async quick_open_activate,
    "quick-open-close" (0, 0) => quick_open_close,
    "wait-quick-open" (0, 1) => async wait_quick_open,
    "report-quick-open" (0, 0) => report_quick_open,
    "hold-ctrl" (0, 0) => hold_ctrl,
    "release-ctrl" (0, 0) => async release_ctrl,
    "switcher-key" (1, 1) => async switcher_key,
    "mouse-button" (1, 1) => async mouse_button,
    "wait-rif" (0, 0) => async wait_rif,
    "report-rif" (0, 0) => report_rif,
    "assert-quick-open-row" (2, 2) => assert_quick_open_row,
    "assert-quick-open-contains" (1, 1) => assert_quick_open_contains,
    "assert-quick-open-lacks" (1, 1) => assert_quick_open_lacks,
    "assert-quick-open-count" (1, 1) => assert_quick_open_count,
    "assert-quick-open-positions" (2, 2) => assert_quick_open_positions,
    "assert-quick-open-files" (1, 1) => assert_quick_open_files,
    "assert-quick-open-note" (1, 1) => assert_quick_open_note,
    "assert-quick-open-shown" (1, 1) => assert_quick_open_shown,
    "assert-switcher" (1, 2) => assert_switcher,
    "assert-switching" (1, 1) => assert_switching,
    "assert-recent-order" (1, 1) => assert_recent_order,
    "assert-places" (2, 2) => assert_places,
    "assert-pinned" (2, 2) => assert_pinned,
    "assert-tab-icon" (2, 2) => assert_tab_icon,
    "assert-tab-menu-shown" (2, 2) => assert_tab_menu_shown,
    "assert-save-name" (1, 1) => assert_save_name,
    "assert-rif" (2, 2) => assert_rif,
];

fn ms(duration: Option<Duration>) -> String {
    duration.map_or_else(
        || "-".to_owned(),
        |duration| format!("{:.1} ms", duration.as_secs_f64() * 1e3),
    )
}

impl Harness {
    /// `quick-open [<text>]`: opens quick open with the text, as Ctrl+P does, and waits until
    /// it is on screen.
    async fn quick_open(&self, args: &[String]) -> StepResult {
        self.window
            .open_quick_open(args.first().map_or("", String::as_str));
        self.wait_quick_open_shown().await
    }

    /// `quick-open-in <folder> [<text>]`: as `quick-open`, listing the project of that folder
    /// (`~/` is the home folder) instead of the current document's.
    async fn quick_open_in(&self, args: &[String]) -> StepResult {
        let folder = match args[0].strip_prefix("~/") {
            Some(rest) => std::env::home_dir().ok_or("no home folder")?.join(rest),
            None => PathBuf::from(&args[0]),
        };
        self.window
            .open_quick_open_from(args.get(1).map_or("", String::as_str), Some(folder));
        self.wait_quick_open_shown().await
    }

    async fn wait_quick_open_shown(&self) -> StepResult {
        let started = Instant::now();
        while !self.window.quick_open_shown() {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err("quick open never opened".to_owned());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        Ok(())
    }

    /// `quick-open-type <text>`: types the text into quick open's field.
    fn quick_open_type(&self, args: &[String]) -> StepResult {
        self.field("quick-open")?.set_text(&args[0]);
        Ok(())
    }

    /// `quick-open-select <row>`: selects a row of quick open.
    fn quick_open_select(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        self.window.quick_open_select(index)
    }

    /// `quick-open-activate`: opens the selected row and waits for its work.
    async fn quick_open_activate(&self, _: &[String]) -> StepResult {
        self.window.quick_open_activate();
        self.wait_idle().await
    }

    /// `quick-open-close`: what Escape does in quick open's field.
    fn quick_open_close(&self, _: &[String]) -> StepResult {
        self.field("quick-open")?
            .emit_by_name::<()>("stop-search", &[]);
        Ok(())
    }

    /// `wait-quick-open [<seconds>]`: waits until quick open has settled: its walk ended and
    /// its rows ranked (15 s unless given).
    async fn wait_quick_open(&self, args: &[String]) -> StepResult {
        let limit = args
            .first()
            .map(|seconds| number(seconds).map(|seconds| seconds as u64))
            .transpose()?
            .map_or(WAIT_TIMEOUT, Duration::from_secs);
        let started = Instant::now();
        while !self.window.quick_open_settled() {
            if started.elapsed() > limit {
                return Err(format!("quick open did not settle in {limit:?}"));
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        Ok(())
    }

    /// `report-quick-open`: prints quick open's timings: first rows, the walk, the slowest
    /// ranking and the last query's.
    fn report_quick_open(&self, _: &[String]) {
        let stats = self.window.quick_open_stats();
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
    }

    /// `hold-ctrl`: Ctrl pressed, through the window's key controller.
    fn hold_ctrl(&self, _: &[String]) {
        self.emit_window_key("key-pressed", "Control_L");
    }

    /// `release-ctrl`: Ctrl released, through the window's key controller; lets the main loop
    /// settle.
    async fn release_ctrl(&self, _: &[String]) {
        self.emit_window_key("key-released", "Control_L");
        self.wait_until_settled().await;
    }

    /// `switcher-key <key name>`: the key pressed with Ctrl held, through the window's key
    /// controller; lets the main loop settle.
    async fn switcher_key(&self, args: &[String]) {
        self.emit_window_key("key-pressed", &args[0]);
        self.wait_until_settled().await;
    }

    /// `mouse-button 8|9`: what the mouse's back (8) and forward (9) buttons do; waits for its
    /// work.
    async fn mouse_button(&self, args: &[String]) -> StepResult {
        let back = match args[0].as_str() {
            "8" => true,
            "9" => false,
            other => return Err(format!("button {other}: use 8 (back) or 9 (forward)")),
        };
        self.window.mouse_navigation(back);
        self.wait_idle().await
    }

    /// `wait-rif`: waits until Replace in Files has finished, and for the work after it.
    async fn wait_rif(&self, _: &[String]) -> StepResult {
        let started = Instant::now();
        while self.window.replace_in_files_running() {
            if started.elapsed() > super::IDLE_TIMEOUT {
                return Err("Replace in Files did not finish".to_owned());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        self.wait_idle().await
    }

    /// `report-rif`: prints the last Replace in Files' counts and timings (on the worker, and
    /// in all).
    fn report_rif(&self, _: &[String]) -> StepResult {
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

    /// Emits a key event on the window's switcher controller, with Ctrl held for every key but
    /// Ctrl's own press.
    fn emit_window_key(&self, signal: &str, key: &str) {
        let keyval = gtk::gdk::Key::from_name(key).unwrap_or(gtk::gdk::Key::VoidSymbol);
        let state = if signal == "key-released" || key != "Control_L" {
            gtk::gdk::ModifierType::CONTROL_MASK
        } else {
            gtk::gdk::ModifierType::empty()
        };
        let values: [&dyn ToValue; 3] = [&keyval.into_glib(), &0u32, &state];
        let keys = self.window.window_keys();
        if signal == "key-pressed" {
            keys.emit_by_name::<bool>(signal, &values);
        } else {
            keys.emit_by_name::<()>(signal, &values);
        }
    }

    /// `assert-quick-open-row <row> <label>`: quick open's row (0 is the first).
    fn assert_quick_open_row(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let labels = self.window.quick_open_labels();
        equal(
            &format!("quick open row {index}"),
            labels.get(index).cloned().unwrap_or_default(),
            args[1].clone(),
        )
    }

    /// `assert-quick-open-contains <label>`: quick open shows a row with that label.
    fn assert_quick_open_contains(&self, args: &[String]) -> StepResult {
        self.quick_open_shows(&args[0], true)
    }

    /// `assert-quick-open-lacks <label>`: quick open shows no row with that label.
    fn assert_quick_open_lacks(&self, args: &[String]) -> StepResult {
        self.quick_open_shows(&args[0], false)
    }

    fn quick_open_shows(&self, label: &str, wanted: bool) -> StepResult {
        let labels = self.window.quick_open_labels();
        if labels.iter().any(|shown| shown == label) == wanted {
            Ok(())
        } else {
            Err(format!("quick open shows {labels:?}"))
        }
    }

    /// `assert-quick-open-count <n>`: how many rows quick open shows.
    fn assert_quick_open_count(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[0])?;
        equal("rows", self.window.quick_open_labels().len(), wanted)
    }

    /// `assert-quick-open-positions <row> <positions>`: the matched characters of a row,
    /// joined by commas.
    fn assert_quick_open_positions(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let positions = self
            .window
            .quick_open_positions(index)
            .ok_or_else(|| format!("there is no row {index}"))?;
        let shown = positions
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",");
        equal("matched characters", shown, args[1].clone())
    }

    /// `assert-quick-open-files <n>`: how many files quick open's project has.
    fn assert_quick_open_files(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[0])?;
        equal(
            "project files",
            self.window.quick_open_stats().files,
            wanted,
        )
    }

    /// `assert-quick-open-note <text>`: quick open's note has the text.
    fn assert_quick_open_note(&self, args: &[String]) -> StepResult {
        let note = self.window.quick_open_note();
        if note.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("the note {note:?} lacks {:?}", args[0]))
        }
    }

    /// `assert-quick-open-shown true|false`: whether quick open is on screen.
    fn assert_quick_open_shown(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal("quick open shown", self.window.quick_open_shown(), wanted)
    }

    /// `assert-switcher true|false [<selected>]`: whether the Ctrl+Tab switcher shows, and the
    /// name of its selected row.
    fn assert_switcher(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal("switcher shown", self.window.switcher_shown(), wanted)?;
        if let Some(selected) = args.get(1) {
            let (_, current) = self.window.switcher_rows();
            equal("selected", current.unwrap_or_default(), selected.clone())?;
        }
        Ok(())
    }

    /// `assert-switching true|false`: whether a Ctrl+Tab switch is under way.
    fn assert_switching(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal("switching", self.window.switching(), wanted)
    }

    /// `assert-recent-order <names>`: the tabs in most-recently-used order, joined by `|`.
    fn assert_recent_order(&self, args: &[String]) -> StepResult {
        equal(
            "recently used order",
            self.window.recent_order().join("|"),
            args[0].clone(),
        )
    }

    /// `assert-places <places> <current>`: Go Back's places as `name:line` joined by `|`, and
    /// the index of the current one or `-`.
    fn assert_places(&self, args: &[String]) -> StepResult {
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
            format!("{} ({})", args[0], args[1]),
        )
    }

    /// `assert-pinned <n> true|false`: whether tab n is pinned.
    fn assert_pinned(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let pages = self.window.pages();
        let page = pages
            .get(index.saturating_sub(1))
            .ok_or_else(|| format!("there is no tab {index}"))?;
        equal("pinned", self.window.is_pinned(page), boolean(&args[1])?)
    }

    /// `assert-tab-icon <n> <icon name>|none`: tab n's icon.
    fn assert_tab_icon(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let (_, tab) = self
            .window
            .tab_at(index)
            .ok_or_else(|| format!("there is no tab {index}"))?;
        let icon = tab
            .icon()
            .map_or_else(|| "none".to_owned(), |icon| icon_name(&icon));
        equal("tab icon", icon, args[1].clone())
    }

    /// `assert-tab-menu-shown <label> true|false`: whether the tab menu shows that item
    /// enabled.
    fn assert_tab_menu_shown(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[1])?;
        let items = self.window.tab_menu_items(0);
        let shown = items.iter().any(|(label, action)| {
            *label == args[0]
                && stet_domain::actions::ActionId::from_name(action)
                    .and_then(|id| self.window.action(id))
                    .is_some_and(|action| action.is_enabled())
        });
        equal(&format!("tab menu shows {}", args[0]), shown, wanted)
    }

    /// `assert-save-name <name>`: the name Save As proposes for the current tab.
    fn assert_save_name(&self, args: &[String]) -> StepResult {
        let name = Window::save_as_name(&self.page()?);
        equal("Save As name", name, args[0].clone())
    }

    /// `assert-rif <field> <value>`: a figure of the last Replace in Files: `replacements`,
    /// `files`, `skipped`, `failed`, `open`, `searched` or `cancelled`.
    fn assert_rif(&self, args: &[String]) -> StepResult {
        let stats = self
            .window
            .replace_in_files_stats()
            .ok_or_else(|| "Replace in Files has not run".to_owned())?;
        let value = match args[0].as_str() {
            "replacements" => stats.replacements.to_string(),
            "files" => stats.files.to_string(),
            "skipped" => stats.skipped.to_string(),
            "failed" => stats.failed.to_string(),
            "open" => stats.open_documents.to_string(),
            "searched" => stats.summary.files_searched.to_string(),
            "cancelled" => stats.summary.cancelled.to_string(),
            other => {
                return Err(format!(
                    "no field {other}; use replacements, files, skipped, failed, open, \
                     searched or cancelled"
                ));
            }
        };
        equal(&args[0], value, args[1].clone())
    }
}

/// A themed icon's first name.
fn icon_name(icon: &gio::Icon) -> String {
    icon.downcast_ref::<gio::ThemedIcon>()
        .and_then(|themed| themed.names().first().map(ToString::to_string))
        .unwrap_or_else(|| "other".to_owned())
}
