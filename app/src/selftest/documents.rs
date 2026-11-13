//! Self-test commands for the documents and their tabs: opening files as a command line,
//! a second `stet` or a drop does, saving, switching tabs, the banners and the encoding
//! picker, and what the tabs, the title and the status bar show.

use super::registry::{Command, MANY, commands};
use super::script::boolean;
use super::{Harness, StepResult, equal, number};
use crate::config::APP_ID_ENV;
use crate::runtime::worker;
use crate::window::PickerMode;
use gtk4::gio;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command as Process;

pub(super) const COMMANDS: &[Command] = commands![
    "open" (1, MANY) => async open,
    "open-start" (1, MANY) => async open_start,
    "spawn-stet" (1, MANY) => async spawn_stet,
    "spawn-stet-expect" (2, MANY) => async spawn_stet_expect,
    "drop" (1, MANY) => async drop_paths,
    "save-as" (1, 1) => async save_as,
    "select-tab" (1, 1) => select_tab,
    "banner-click" (1, 1) => async banner_click,
    "window-focus" (0, 0) => async window_focus,
    "unwatch" (0, 0) => unwatch,
    "pick-encoding" (2, 2) => async pick_encoding,
    "choose-language" (1, 1) => choose_language,
    "assert-tab-count" (1, 1) => assert_tab_count,
    "assert-title" (1, 1) => assert_title,
    "assert-tab-title" (2, 2) => assert_tab_title,
    "assert-tab-text" (2, 2) => assert_tab_text,
    "assert-language" (1, 1) => assert_language,
    "assert-dirty" (1, 1) => assert_dirty,
    "assert-read-only" (1, 1) => assert_read_only,
    "assert-banner" (1, 1) => assert_banner,
    "assert-no-banner" (1, 1) => assert_no_banner,
    "assert-highlight" (1, 1) => assert_highlight,
    "assert-large" (1, 1) => assert_large,
    "assert-editable" (1, 1) => assert_editable,
    "assert-recent" (1, 1) => assert_recent,
    "assert-closed" (1, 1) => assert_closed,
    "assert-toast" (1, 1) => assert_toast,
    "assert-status" (1, 1) => assert_status,
];

impl Harness {
    /// `open <arguments…>`: opens files as a command line in the running instance would
    /// (`file:line:col`, `-n`, `-c`) and waits until they are loaded.
    async fn open(&self, args: &[String]) -> StepResult {
        self.open_command_line(args).await?;
        self.wait_idle().await
    }

    /// `open-start <arguments…>`: as `open`, without waiting for the loads.
    async fn open_start(&self, args: &[String]) -> StepResult {
        self.open_command_line(args).await
    }

    async fn open_command_line(&self, args: &[String]) -> StepResult {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let parsed = crate::cli::parse(std::iter::once(OsString::from("stet")).chain(args))
            .map_err(|error| error.to_string())?;
        self.window
            .open_command_line(self.root.clone(), parsed.files, parsed.line, parsed.column)
            .await;
        Ok(())
    }

    /// `spawn-stet <arguments…>`: runs a second `stet` in `$TMP` with the harness's id and
    /// waits for it to exit with status 0.
    async fn spawn_stet(&self, args: &[String]) -> StepResult {
        self.run_second_stet(args, 0).await
    }

    /// `spawn-stet-expect <status> <arguments…>`: as `spawn-stet`, expecting that exit status.
    async fn spawn_stet_expect(&self, args: &[String]) -> StepResult {
        let status = args[0]
            .parse::<i32>()
            .map_err(|_| format!("expected an exit status, got {}", args[0]))?;
        self.run_second_stet(&args[1..], status).await
    }

    /// Runs a second `stet` with the harness's application id, as a user in another terminal
    /// would; it forwards its command line to this instance and exits with `expected`.
    async fn run_second_stet(&self, args: &[String], expected: i32) -> StepResult {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let cwd = self.root.clone();
        let app_id = self.app_id.clone();
        let args = args.to_vec();
        let output = worker::run(move || {
            Process::new(exe)
                .args(&args)
                .current_dir(cwd)
                .env(APP_ID_ENV, app_id)
                .output()
        })
        .await
        .ok_or("the worker failed")?
        .map_err(|error| error.to_string())?;
        if output.status.code() != Some(expected) {
            return Err(format!(
                "stet exited with {}, wanted {expected}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    /// `drop <paths…>`: calls the window's file-drop handler with those files and waits until
    /// they are loaded.
    async fn drop_paths(&self, args: &[String]) -> StepResult {
        let files = args.iter().map(gio::File::for_path).collect();
        if !self.window.drop_files(files) {
            return Err("the drop was refused".to_owned());
        }
        self.wait_idle().await
    }

    /// `save-as <path>`: saves the current tab to the path (the file chooser is not used).
    async fn save_as(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        if self.window.save_to(&page, PathBuf::from(&args[0])).await {
            Ok(())
        } else {
            Err("the save failed".to_owned())
        }
    }

    /// `select-tab <n>`: selects tab n, counted across the views.
    fn select_tab(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let pages = self.window.pages();
        let page = pages
            .get(index.wrapping_sub(1))
            .ok_or_else(|| format!("there is no tab {index}"))?;
        self.window.select(page);
        Ok(())
    }

    /// `banner-click <label>`: presses the current tab's banner button with that label, as the
    /// keyboard does with the focus on it.
    async fn banner_click(&self, args: &[String]) -> StepResult {
        if self.page()?.click_banner(&args[0]) {
            self.wait_until_settled().await;
            Ok(())
        } else {
            Err(format!(
                "no banner button {:?}; the banners have {:?}",
                args[0],
                self.page()?.banner_buttons()
            ))
        }
    }

    /// `window-focus`: runs the check of every open file that the window's focus triggers.
    async fn window_focus(&self, _: &[String]) -> StepResult {
        self.window.check_all_files();
        self.wait_idle().await
    }

    /// `unwatch`: drops the current tab's file monitor, as on a file system that sends no
    /// change events.
    fn unwatch(&self, _: &[String]) -> StepResult {
        self.page()?.set_watch(None);
        Ok(())
    }

    /// `pick-encoding reinterpret|convert <encoding>`: chooses an encoding in the status
    /// bar's picker and waits for the result.
    async fn pick_encoding(&self, args: &[String]) -> StepResult {
        let mode = match args[0].as_str() {
            "reinterpret" => PickerMode::Reinterpret,
            "convert" => PickerMode::Convert,
            other => return Err(format!("{other}: use reinterpret or convert")),
        };
        self.window.update_status();
        if !self.window.pick_encoding(mode, &args[1]) {
            return Err(format!("the picker can't {} {}", args[0], args[1]));
        }
        self.wait_idle().await
    }

    /// `choose-language <query>`: chooses the best match in the language picker.
    fn choose_language(&self, args: &[String]) -> StepResult {
        if self.window.pick_language(&args[0]) {
            Ok(())
        } else {
            Err(format!("no language matches {}", args[0]))
        }
    }

    /// `assert-tab-count <n>`: the window has n tabs.
    fn assert_tab_count(&self, args: &[String]) -> StepResult {
        equal("tabs", self.window.pages().len(), number(&args[0])?)
    }

    /// `assert-title <title>`: the window's title.
    fn assert_title(&self, args: &[String]) -> StepResult {
        equal("title", self.window.title(), args[0].clone())
    }

    /// `assert-tab-title <n> <title>`: tab n's title.
    fn assert_tab_title(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let (_, tab) = self
            .window
            .tab_at(index)
            .ok_or_else(|| format!("there is no tab {index}"))?;
        equal("tab title", tab.title().to_string(), args[1].clone())
    }

    /// `assert-tab-text <n> <text>`: tab n's text.
    fn assert_tab_text(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let pages = self.window.pages();
        let page = pages
            .get(index.saturating_sub(1))
            .ok_or_else(|| format!("there is no tab {index}"))?;
        equal("text", page.text(), args[1].clone())
    }

    /// `assert-language <id>|none`: the current tab's language.
    fn assert_language(&self, args: &[String]) -> StepResult {
        let language = self
            .page()?
            .language()
            .map_or_else(|| "none".to_owned(), |language| language.id().to_string());
        equal("language", language, args[0].clone())
    }

    /// `assert-dirty true|false`: whether the current tab has unsaved changes.
    fn assert_dirty(&self, args: &[String]) -> StepResult {
        equal("dirty", self.page()?.is_dirty(), boolean(&args[0])?)
    }

    /// `assert-read-only true|false`: whether the current tab is read-only.
    fn assert_read_only(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal(
            "read-only",
            page.read_only().is_some() || !page.view().is_editable(),
            boolean(&args[0])?,
        )
    }

    /// `assert-banner <text>|none`: a banner of the current tab has the text, or none shows.
    fn assert_banner(&self, args: &[String]) -> StepResult {
        let banners = self.page()?.banner_texts();
        match args[0].as_str() {
            "none" if banners.is_empty() => Ok(()),
            "none" => Err(format!("banners show {banners:?}")),
            wanted if banners.iter().any(|text| text.contains(wanted)) => Ok(()),
            wanted => Err(format!("no banner has {wanted:?}; banners: {banners:?}")),
        }
    }

    /// `assert-no-banner <text>`: no banner of the current tab has the text.
    fn assert_no_banner(&self, args: &[String]) -> StepResult {
        let banners = self.page()?.banner_texts();
        match banners.iter().find(|text| text.contains(args[0].as_str())) {
            Some(text) => Err(format!("a banner shows {text:?}")),
            None => Ok(()),
        }
    }

    /// `assert-highlight true|false`: whether the current tab highlights its language.
    fn assert_highlight(&self, args: &[String]) -> StepResult {
        equal(
            "highlighting",
            self.page()?.buffer().is_highlight_syntax()
                && self.page()?.buffer().language().is_some(),
            boolean(&args[0])?,
        )
    }

    /// `assert-large true|false`: whether the current tab is in large-file mode.
    fn assert_large(&self, args: &[String]) -> StepResult {
        equal(
            "large-file mode",
            self.page()?.large_file_mode(),
            boolean(&args[0])?,
        )
    }

    /// `assert-editable true|false`: whether the current tab's view is editable.
    fn assert_editable(&self, args: &[String]) -> StepResult {
        equal(
            "editable",
            self.page()?.view().is_editable(),
            boolean(&args[0])?,
        )
    }

    /// `assert-recent <path>`: the most recent file.
    fn assert_recent(&self, args: &[String]) -> StepResult {
        let recent = self.window.shared().recent();
        let first = recent.paths().first().cloned().unwrap_or_default();
        equal("most recent file", first, PathBuf::from(&args[0]))
    }

    /// `assert-closed <n>`: Restore Closed Tab has n tabs to restore.
    fn assert_closed(&self, args: &[String]) -> StepResult {
        equal("closed tabs", self.window.closed_count(), number(&args[0])?)
    }

    /// `assert-toast <text>`: the last toast has the text.
    fn assert_toast(&self, args: &[String]) -> StepResult {
        let toast = self.window.last_toast();
        if toast.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("the last toast {toast:?} lacks {:?}", args[0]))
        }
    }

    /// `assert-status <text>`: the status bar has the text.
    fn assert_status(&self, args: &[String]) -> StepResult {
        let status = self.window.status_text();
        if status.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("status {status:?} lacks {:?}", args[0]))
        }
    }
}
