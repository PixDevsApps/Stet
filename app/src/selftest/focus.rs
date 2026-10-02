//! Self-test commands for the keyboard focus: which widget of the window has it, and the
//! popovers whose closing must give it back to the editor.

use super::script::Step;
use super::{Harness, StepResult, WAIT_TIMEOUT, equal};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::path::Path;
use std::time::{Duration, Instant};

impl Harness {
    /// Runs a focus command; `None` when `step` is not one.
    pub(super) async fn focus_step(&self, step: &Step) -> Option<StepResult> {
        let arg = |index: usize| step.args[index].as_str();
        Some(match step.command.as_str() {
            // GTK moves a focus whose widget was hidden or removed only after the next painted
            // frame, so the check waits for two frames, and then for idle work, first.
            "assert-focus" => match self.wait_frames(2).await {
                Ok(()) => {
                    self.wait_until_settled().await;
                    self.check_focus(arg(0))
                }
                Err(error) => Err(error),
            },
            // What libadwaita's tab strip does for a click on a tab that is not selected:
            // select its page, then give the page's child the focus.
            "click-tab" => match super::number(arg(0)) {
                Ok(index) => match self.window.tab_at(index) {
                    Some((tabs, tab)) => {
                        tabs.set_selected_page(&tab);
                        tab.child().grab_focus();
                        Ok(())
                    }
                    None => Err(format!("there is no tab {index}")),
                },
                Err(error) => Err(error),
            },
            "choose-file" => self.choose_file(Path::new(arg(0))).await,
            "assert-chooser-name" => self.chooser_name(arg(0)).await,
            "popup" => match arg(0) {
                "palette" => {
                    self.window.open_palette("");
                    Ok(())
                }
                name => self.menu_button(name).map(|button| button.popup()),
            },
            "popdown" => match arg(0) {
                "palette" => {
                    self.window.palette().popover.popdown();
                    Ok(())
                }
                name => self.menu_button(name).map(|button| button.popdown()),
            },
            _ => return None,
        })
    }

    /// Picks `path` in the file chooser that Open or Save As shows and accepts it. Without a
    /// portal (headless) GTK shows its own chooser dialog in this process; it is driven through
    /// the deprecated chooser interface, which that dialog still implements.
    #[allow(deprecated)]
    async fn choose_file(&self, path: &Path) -> StepResult {
        let started = Instant::now();
        let chooser = open_chooser().await?;
        let refused =
            |error: glib::Error| format!("the chooser refused {}: {error}", path.display());
        if chooser.action() == gtk::FileChooserAction::Save {
            // GTK 4.22's Save chooser keeps its suggested name when given a file.
            let folder = path.parent().map(gio::File::for_path);
            chooser
                .set_current_folder(folder.as_ref())
                .map_err(refused)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            chooser.set_current_name(&name);
        } else {
            chooser
                .set_file(&gio::File::for_path(path))
                .map_err(refused)?;
        }
        // The chooser selects the file once it has listed the folder.
        let chosen = |chooser: &gtk::FileChooserDialog| {
            chooser
                .files()
                .iter::<gio::File>()
                .flatten()
                .any(|chosen| chosen.path().as_deref() == Some(path))
        };
        while !chosen(&chooser) {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err(format!("the chooser never selected {}", path.display()));
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        chooser.response(gtk::ResponseType::Accept);
        Ok(())
    }

    /// The name the open Save As chooser proposes.
    #[allow(deprecated)]
    async fn chooser_name(&self, wanted: &str) -> StepResult {
        let chooser = open_chooser().await?;
        let proposed = chooser.current_name().map(String::from).unwrap_or_default();
        equal("the proposed name", proposed, wanted.to_owned())
    }

    fn menu_button(&self, name: &str) -> Result<gtk::MenuButton, String> {
        let status = &self.window.inner.status;
        match name {
            "language" => Ok(status.language.clone()),
            "eol" => Ok(status.eol.clone()),
            "encoding" => Ok(status.encoding.clone()),
            "indentation" => Ok(status.indentation.clone()),
            "menu" => Ok(self.window.menu_button().clone()),
            other => Err(format!(
                "no popover {other}; use palette, language, eol, encoding, indentation or menu"
            )),
        }
    }

    /// Whether the window's focus widget is `target`, or inside it, and on screen.
    pub(super) fn check_focus(&self, target: &str) -> StepResult {
        let inner = &self.window.inner;
        let focus = gtk::prelude::RootExt::focus(self.window.window());
        let within = |widget: &gtk::Widget| {
            focus
                .as_ref()
                .is_some_and(|focus| focus == widget || focus.is_ancestor(widget))
        };
        let found = match target {
            "editor" => {
                let page = self.page()?;
                focus.as_ref() == Some(page.view().upcast_ref::<gtk::Widget>())
            }
            "find" => within(inner.find.entry.upcast_ref()),
            "replace" => within(inner.find.replace_entry.upcast_ref()),
            "results" => within(inner.results.root.upcast_ref()),
            "palette" => within(self.window.palette().popover.upcast_ref()),
            "quick-open" => within(self.window.inner.nav.quick_open.popover.upcast_ref()),
            "language" => within(inner.languages.popover.upcast_ref()),
            "encoding" => within(inner.encodings.popover.upcast_ref()),
            "indentation" => within(inner.indentation.popover.upcast_ref()),
            "dialog" => self
                .window
                .open_dialog()
                .is_some_and(|dialog| within(dialog.upcast_ref())),
            other => {
                return Err(format!(
                    "no focus target {other}; use editor, find, replace, results, palette, \
                     quick-open, language, encoding or dialog"
                ));
            }
        };
        let shown = focus.as_ref().is_some_and(WidgetExt::is_mapped);
        if found && shown {
            Ok(())
        } else {
            Err(format!("the focus is on {}", describe(focus.as_ref())))
        }
    }
}

/// The file chooser that is open, once it shows (GTK's own dialog, headless).
#[allow(deprecated)]
async fn open_chooser() -> Result<gtk::FileChooserDialog, String> {
    let started = Instant::now();
    loop {
        let open = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk::FileChooserDialog>().ok())
            .find(|dialog| dialog.is_visible());
        if let Some(chooser) = open {
            return Ok(chooser);
        }
        if started.elapsed() > WAIT_TIMEOUT {
            return Err("no file chooser opened".to_owned());
        }
        glib::timeout_future(Duration::from_millis(20)).await;
    }
}

/// The focus widget and its nearest ancestors, for a failure message: `GtkButton “Plain
/// Text” in GtkMenuButton in GtkBox.stet-statusbar`.
fn describe(widget: Option<&gtk::Widget>) -> String {
    let Some(widget) = widget else {
        return "nothing".to_owned();
    };
    let mut parts = Vec::new();
    let mut current = Some(widget.clone());
    while let Some(widget) = current.take() {
        if parts.len() == 5 || widget.is::<gtk::Window>() {
            break;
        }
        let mut part = widget.type_().name().to_owned();
        for class in widget.css_classes() {
            part.push('.');
            part.push_str(&class);
        }
        let label = widget
            .downcast_ref::<gtk::Button>()
            .and_then(|button| button.label())
            .or_else(|| {
                widget
                    .downcast_ref::<gtk::Label>()
                    .map(|label| label.label())
            });
        if let Some(label) = label.filter(|label| !label.is_empty()) {
            part.push_str(&format!(" “{label}”"));
        }
        parts.push(part);
        current = widget.parent();
    }
    let mapped = if widget.is_mapped() {
        ""
    } else {
        " (not on screen)"
    };
    format!("{}{mapped}", parts.join(" in "))
}
