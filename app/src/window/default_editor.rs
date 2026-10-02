//! Set as Default Editor… (M5, INTEGRATIONS.md section 4): an opt-in command that says what it
//! will change and asks first, then writes `stet` to Omarchy's default-editor file, so
//! SUPER+SHIFT+N (`omarchy-launch-editor`) starts Stet, and makes Stet the default for the
//! text types in its desktop entry with `xdg-mime`, so a double-click in the file manager
//! opens Stet. It is the only place where Stet changes the user's configuration.

use super::Window;
use crate::worker;
use std::path::{Path, PathBuf};
use stet_domain::document::tilde;
use stet_infrastructure::desktop::{self, COMMAND, DESKTOP_ID};
use stet_infrastructure::omarchy::defaults;

/// What the system looks like before the change.
struct Before {
    editor_file: PathBuf,
    editor: Option<String>,
    plain_text: Option<String>,
    on_path: bool,
    desktop_file: Option<PathBuf>,
    types: Vec<&'static str>,
}

/// What the change did.
struct After {
    plain_text: Option<String>,
}

impl Window {
    pub(super) async fn set_as_default_editor(&self) {
        let omarchy_dir = self.inner.shared.config.omarchy_dir.clone();
        let probe = worker::run(move || Before {
            editor_file: defaults::editor_file(&omarchy_dir),
            editor: defaults::read_default_editor(&omarchy_dir).ok().flatten(),
            plain_text: desktop::default_for("text/plain").ok().flatten(),
            on_path: desktop::find_program(COMMAND).is_some(),
            desktop_file: desktop::find_desktop_file(DESKTOP_ID),
            types: desktop::mime_types(),
        })
        .await;
        let Some(before) = probe else {
            self.toast("Could not look up the current default editor");
            return;
        };
        let home = std::env::home_dir();
        let shown = |path: &Path| tilde(path, home.as_deref());
        let mut body = format!(
            "SUPER+SHIFT+N: writes “{COMMAND}” to {} (now: {}).\n\nDouble-click in the file \
             manager: runs xdg-mime default {DESKTOP_ID} for {} file types, text/plain among \
             them (now: {}).",
            shown(&before.editor_file),
            before
                .editor
                .as_deref()
                .unwrap_or("nvim, Omarchy's default"),
            before.types.len(),
            before.plain_text.as_deref().unwrap_or("none"),
        );
        if !before.on_path {
            body.push_str(&format!(
                "\n\n“{COMMAND}” is not on PATH, so omarchy-launch-editor would fall back to \
                 nvim until Stet is installed."
            ));
        }
        if before.desktop_file.is_none() {
            body.push_str(&format!(
                "\n\n{DESKTOP_ID} is not installed, so double-clicked files can't open in Stet \
                 until it is."
            ));
        }
        body.push_str(
            "\n\nTo undo: omarchy-default-editor nvim, and xdg-mime default nvim.desktop \
             text/plain (and the other types).",
        );
        let confirmed = self
            .confirm(
                "Make Stet the default editor?",
                &body,
                "_Set as Default",
                false,
            )
            .await;
        if !confirmed {
            return;
        }
        let omarchy_dir = self.inner.shared.config.omarchy_dir.clone();
        let types = before.types.clone();
        let applied = worker::run(move || -> Result<After, String> {
            defaults::write_default_editor(&omarchy_dir, COMMAND)
                .map_err(|error| format!("Could not write the default-editor file: {error}"))?;
            desktop::set_default(DESKTOP_ID, &types).map_err(|error| error.to_string())?;
            Ok(After {
                plain_text: desktop::default_for("text/plain").ok().flatten(),
            })
        })
        .await;
        match applied {
            Some(Ok(after)) => {
                let report = format!(
                    "{} now holds “{COMMAND}” (it held {}), so SUPER+SHIFT+N opens Stet.\n\n\
                     {} file types open with {DESKTOP_ID}; xdg-mime now reports {} for \
                     text/plain (it reported {}).",
                    shown(&before.editor_file),
                    before
                        .editor
                        .as_deref()
                        .map_or_else(|| "nothing".to_owned(), |editor| format!("“{editor}”")),
                    before.types.len(),
                    after.plain_text.as_deref().unwrap_or("nothing"),
                    before.plain_text.as_deref().unwrap_or("nothing"),
                );
                tracing::info!(types = before.types.len(), "set as the default editor");
                self.show_message("Stet is the default editor", &report)
                    .await;
                self.toast("Stet is the default editor");
            }
            Some(Err(error)) => {
                self.show_error("Stet could not be made the default editor", &error)
                    .await;
            }
            None => self.toast("Setting the default editor failed"),
        }
    }
}
