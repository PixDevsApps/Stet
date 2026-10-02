//! `keys.toml`: the user's changes to the default keys (ADR-009, ADR-017). Each line names an
//! action and lists its keys, which replace the action's default keys; `[]` unbinds it. A key
//! may belong to one action only, GtkTextView's own editing keys included.

use std::collections::HashMap;
use std::ops::Range;

use toml::de::DeValue;

use super::{FileError, closest, entries, parse_toml};
use crate::actions::{Accelerator, ActionId, ActionKind, KeyScope, Menu};

/// The registry's keys, with the user's lists in place of an action's own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    /// Keys in their canonical spelling, as GTK's accelerator parser reads them.
    overrides: HashMap<ActionId, Vec<String>>,
}

/// Turns a key as written into its canonical spelling, or `None` when it is no key. The app
/// uses GTK's parser; tests use [`Accelerator::parse`].
pub type Canonical<'a> = &'a dyn Fn(&str) -> Option<String>;

impl Keymap {
    /// Every key of `id`, GtkTextView's own included; the first is the one menus show.
    pub fn keys(&self, id: ActionId) -> Vec<String> {
        match self.overrides.get(&id) {
            Some(keys) => keys.clone(),
            None => id.keys().all().map(str::to_owned).collect(),
        }
    }

    /// The keys Stet installs for `id`, where its [`KeyScope`] says.
    pub fn installed(&self, id: ActionId) -> Vec<String> {
        match self.overrides.get(&id) {
            Some(keys) => keys.clone(),
            None => id.keys().app.iter().map(|key| (*key).to_owned()).collect(),
        }
    }

    /// The application accelerators of a window-scoped action.
    pub fn accels(&self, id: ActionId) -> Vec<String> {
        match id.key_scope() {
            KeyScope::Window => self.installed(id),
            KeyScope::Editor => Vec::new(),
        }
    }

    /// The editor shortcuts of an editor-scoped action.
    pub fn editor_keys(&self, id: ActionId) -> Vec<String> {
        match id.key_scope() {
            KeyScope::Editor => self.installed(id),
            KeyScope::Window => Vec::new(),
        }
    }

    /// Whether `keys.toml` changed the keys of `id`.
    pub fn is_changed(&self, id: ActionId) -> bool {
        self.overrides.contains_key(&id)
    }

    /// How many actions `keys.toml` changed.
    pub fn changed_count(&self) -> usize {
        self.overrides.len()
    }

    /// Reads `keys.toml`. Every mistake is listed, in the order of the file; a file with
    /// mistakes is not used at all.
    pub fn parse(text: &str, canonical: Canonical<'_>) -> Result<Self, Vec<FileError>> {
        let root = parse_toml(text).map_err(|error| vec![error])?;
        let mut errors = Vec::new();
        let mut lists: Vec<(ActionId, Vec<WrittenKey>)> = Vec::new();
        for (key, value) in entries(root.get_ref()) {
            let name = key.get_ref().as_ref();
            let at = |message: String| FileError::at(text, key.span().start, message);
            let Some(id) = ActionId::from_name(name) else {
                let hint = closest(name, ActionId::ALL.map(ActionId::name))
                    .map(|near| format!("; did you mean {near}?"))
                    .unwrap_or_default();
                errors.push(at(format!("unknown action {name}{hint}")));
                continue;
            };
            if id.kind() == ActionKind::WithString {
                errors.push(at(format!(
                    "{name} takes a parameter from its menu entries and has no keys"
                )));
                continue;
            }
            if id.has_widget_keys() {
                let keys: Vec<String> = id.keys().widget.iter().map(|key| display(key)).collect();
                errors.push(at(format!(
                    "{name} keeps the text view's own keys ({}); they can't be changed",
                    keys.join(", ")
                )));
                continue;
            }
            let Some(array) = value.get_ref().as_array() else {
                errors.push(FileError::at(
                    text,
                    value.span().start,
                    format!("{name} takes a list of keys, like {name} = [\"<Control>d\"]"),
                ));
                continue;
            };
            let mut keys: Vec<WrittenKey> = Vec::new();
            for item in array.iter() {
                let span = item.span();
                let error = |message: String| FileError::at(text, span.start, message);
                let DeValue::String(written) = item.get_ref() else {
                    errors.push(error(format!(
                        "a key is a string in quotes, like \"<Control>d\", not {}",
                        item.get_ref().type_str()
                    )));
                    continue;
                };
                let Some(key) = canonical(written) else {
                    errors.push(error(format!(
                        "“{written}” is not a key; keys look like <Control>d, <Alt><Shift>Up or F3"
                    )));
                    continue;
                };
                if !takes_no_typing(&key) {
                    errors.push(error(format!(
                        "“{written}” has no Ctrl, Alt or Super, so it would take the key from \
                         typing (F-keys are the exception)"
                    )));
                    continue;
                }
                if keys.iter().any(|other| other.key == key) {
                    errors.push(error(format!("“{written}” is listed twice for {name}")));
                    continue;
                }
                keys.push(WrittenKey {
                    key,
                    written: written.to_string(),
                    span,
                });
            }
            lists.push((id, keys));
        }

        let changed: Vec<ActionId> = lists.iter().map(|(id, _)| *id).collect();
        let mut taken: HashMap<String, (ActionId, Option<usize>)> = HashMap::new();
        for id in ActionId::ALL.into_iter().filter(|id| !changed.contains(id)) {
            for key in id.keys().all() {
                if let Some(key) = canonical(key) {
                    taken.entry(key).or_insert((id, None));
                }
            }
        }
        let mut overrides = HashMap::new();
        for (id, keys) in lists {
            let mut installed = Vec::with_capacity(keys.len());
            for WrittenKey { key, written, span } in keys {
                match taken.get(&key) {
                    Some(&(other, None)) if other.has_widget_keys() => {
                        errors.push(FileError::at(
                            text,
                            span.start,
                            format!(
                                "“{written}” is the text view's own key for {}; pick another key",
                                other.label()
                            ),
                        ));
                    }
                    Some(&(other, None)) => errors.push(FileError::at(
                        text,
                        span.start,
                        format!(
                            "“{written}” is already {}'s key ({}); unbind it first with {} = []",
                            other.label(),
                            other.name(),
                            other.name()
                        ),
                    )),
                    Some(&(other, Some(line))) => errors.push(FileError::at(
                        text,
                        span.start,
                        format!(
                            "“{written}” is also given to {} on line {line}",
                            other.name()
                        ),
                    )),
                    None => {
                        let line = FileError::at(text, span.start, "").line;
                        taken.insert(key.clone(), (id, Some(line)));
                        installed.push(key);
                    }
                }
            }
            overrides.insert(id, installed);
        }
        if errors.is_empty() {
            Ok(Self { overrides })
        } else {
            errors.sort_by_key(|error| (error.line, error.column));
            Err(errors)
        }
    }
}

/// A key from the file: its canonical spelling, as written, and where.
struct WrittenKey {
    key: String,
    written: String,
    span: Range<usize>,
}

/// Whether a key leaves typing alone: it has Ctrl, Alt or Super, or it is an F-key.
fn takes_no_typing(canonical: &str) -> bool {
    Accelerator::parse(canonical).map_or_else(
        || {
            ["<Control>", "<Alt>", "<Super>", "<Primary>", "<Meta>"]
                .iter()
                .any(|modifier| canonical.contains(modifier))
        },
        |accel| accel.control || accel.alt || accel.super_key || accel.is_function_key(),
    )
}

fn display(key: &str) -> String {
    Accelerator::parse(key).map_or_else(|| key.to_owned(), |accel| accel.display())
}

/// Whether `keys.toml` can change the keys of `id`.
pub fn is_rebindable(id: ActionId) -> bool {
    id.kind() != ActionKind::WithString && !id.has_widget_keys()
}

/// The `keys.toml` that Open Keyboard Shortcuts creates: every action Stet lets you rebind,
/// commented out with its default keys, grouped by its place in the menu.
pub fn default_keys_file() -> String {
    let mut out = String::from(
        "# Stet keyboard shortcuts (keys.toml): your changes to Stet's default keys.\n\
         #\n\
         # Uncomment a line and change it; Stet applies the file when you save it. A line names\n\
         # an action and lists its keys, which replace the default ones; the first key is the\n\
         # one menus show. [] leaves an action without a key:\n\
         #\n\
         #   duplicate-line = [\"<Control>d\"]\n\
         #   uppercase = [\"<Control><Shift>u\", \"<Alt><Shift>u\"]\n\
         #   quit = []\n\
         #\n\
         # Modifiers: <Control> (or <Ctrl>), <Shift>, <Alt>, <Super>. Keys have GTK's names:\n\
         # a-z, 0-9, F1-F35, Return, Tab, Up, Page_Down, plus, minus, KP_Add, ... A key belongs\n\
         # to one action only: to give an action another one's key, unbind that one first. A\n\
         # key needs Ctrl, Alt or Super unless it is an F-key.\n\
         #\n\
         # Undo, Redo, Cut, Copy, Paste, Delete and Select All keep the text view's own keys.\n\
         # The text tools' keys (Edit, Search's brace and select-and-find commands, Tools) work\n\
         # while the editor has the keyboard focus; the others anywhere in the window.\n\
         # fcitx5 takes Ctrl+Space, Ctrl+Shift+U and Ctrl+Alt+Shift+U before Stet sees them;\n\
         # docs/INTEGRATIONS.md says how to free Ctrl+Shift+U for UPPERCASE.\n",
    );
    let mut groups: Vec<(String, Vec<ActionId>)> = Vec::new();
    let mut add =
        |path: String, id: ActionId| match groups.iter_mut().find(|(group, _)| *group == path) {
            Some((_, ids)) => ids.push(id),
            None => groups.push((path, vec![id])),
        };
    for menu in Menu::ALL {
        for id in ActionId::ALL {
            if id.spec().menu.is_some_and(|place| place.menu == menu) && is_rebindable(id) {
                add(id.menu_path().unwrap_or_default(), id);
            }
        }
    }
    for id in ActionId::ALL {
        if id.spec().menu.is_none() && is_rebindable(id) {
            add("Command palette only".to_owned(), id);
        }
    }
    for (path, ids) in groups {
        out.push_str(&format!("\n# {path}\n"));
        for id in ids {
            let keys: Vec<String> = id
                .keys()
                .app
                .iter()
                .map(|key| format!("\"{key}\""))
                .collect();
            let setting = format!("{} = [{}]", id.name(), keys.join(", "));
            out.push_str(&format!("# {setting:<50} # {}\n", id.label()));
        }
    }
    out
}

/// Where the user guide's keymap table starts and ends; the table between them is
/// [`markdown_table`]'s, which a test keeps in step with the registry.
pub const TABLE_START: &str = "<!-- keymap table: generated from the registry -->";
pub const TABLE_END: &str = "<!-- end of the keymap table -->";

/// The default keymap as a Markdown table for the user guide: every command the palette
/// lists, in menu order, with its keys, where they work and its name in `keys.toml`.
pub fn markdown_table() -> String {
    let mut out = String::from(
        "| Menu | Command | Keys | Keys work | `keys.toml` name |\n| --- | --- | --- | --- | --- |\n",
    );
    let mut ids: Vec<ActionId> = Vec::new();
    for menu in Menu::ALL {
        for id in ActionId::ALL {
            if id.spec().menu.is_some_and(|place| place.menu == menu)
                && id.kind() != ActionKind::WithString
            {
                ids.push(id);
            }
        }
    }
    ids.extend(
        ActionId::ALL
            .into_iter()
            .filter(|id| id.spec().menu.is_none() && id.spec().palette),
    );
    for id in ids {
        let keys: Vec<String> = id.keys().all().map(display).collect();
        let keys = if keys.is_empty() {
            "—".to_owned()
        } else {
            keys.join(", ").replace('|', "\\|")
        };
        let place = id.menu_path().unwrap_or_else(|| "Palette".to_owned());
        let scope = match (id.has_widget_keys(), id.key_scope(), keys.as_str()) {
            (_, _, "—") => "",
            (true, _, _) => "Text fields",
            (false, KeyScope::Editor, _) => "Editor",
            (false, KeyScope::Window, _) => "Window",
        };
        let name = if is_rebindable(id) {
            format!("`{}`", id.name())
        } else {
            "—".to_owned()
        };
        out.push_str(&format!(
            "| {place} | {} | {keys} | {scope} | {name} |\n",
            id.label()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(key: &str) -> Option<String> {
        Accelerator::parse(key).map(|accel| accel.canonical())
    }

    fn parse(text: &str) -> Result<Keymap, Vec<String>> {
        Keymap::parse(text, &canonical)
            .map_err(|errors| errors.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn the_defaults_are_the_registry() {
        let keymap = parse("").unwrap();
        assert_eq!(keymap, Keymap::default());
        assert_eq!(keymap.keys(ActionId::DuplicateLine), ["<Control>d"]);
        assert_eq!(keymap.keys(ActionId::Undo), ["<Control>z"]);
        assert!(keymap.installed(ActionId::Undo).is_empty());
        assert_eq!(keymap.accels(ActionId::Save), ["<Control>s"]);
        assert!(keymap.accels(ActionId::DuplicateLine).is_empty());
        assert_eq!(keymap.editor_keys(ActionId::DuplicateLine), ["<Control>d"]);
        assert!(keymap.editor_keys(ActionId::Save).is_empty());
    }

    #[test]
    fn overrides_replace_and_unbind() {
        let keymap = parse(
            "uppercase = [\"<Ctrl><Shift>U\", \"<Alt><Shift>u\"]\nquit = []\n\
             duplicate-line = [\"<Alt>d\"]\n",
        )
        .unwrap();
        assert_eq!(
            keymap.keys(ActionId::Uppercase),
            ["<Control><Shift>u", "<Shift><Alt>u"]
        );
        assert!(keymap.keys(ActionId::Quit).is_empty());
        assert!(keymap.accels(ActionId::Quit).is_empty());
        assert_eq!(keymap.editor_keys(ActionId::DuplicateLine), ["<Alt>d"]);
        assert!(keymap.is_changed(ActionId::Quit));
        assert!(!keymap.is_changed(ActionId::Save));
        assert_eq!(keymap.changed_count(), 3);
    }

    #[test]
    fn a_freed_key_can_be_reused_wherever_it_is_freed() {
        let keymap =
            parse("toggle-comment = [\"<Control>d\"]\nduplicate-line = [\"<Control><Shift>d\"]\n")
                .unwrap();
        assert_eq!(keymap.keys(ActionId::ToggleComment), ["<Control>d"]);
        assert_eq!(keymap.keys(ActionId::DuplicateLine), ["<Control><Shift>d"]);
    }

    #[test]
    fn conflicts_are_reported_where_they_are() {
        assert_eq!(
            parse("toggle-comment = [\"<Control>d\"]\n").unwrap_err(),
            [
                "line 1, column 19: “<Control>d” is already Duplicate Current Line's key \
                 (duplicate-line); unbind it first with duplicate-line = []"
            ]
        );
        assert_eq!(
            parse("quit = []\nfind = [\"<Alt>F7\"]\nsave = [\"<Alt>F7\"]\n").unwrap_err(),
            ["line 3, column 9: “<Alt>F7” is also given to find on line 2"]
        );
        // The text view's own keys are taken too.
        assert_eq!(
            parse("find = [\"<Control>z\"]\n").unwrap_err(),
            [
                "line 1, column 9: “<Control>z” is the text view's own key for Undo; pick another key"
            ]
        );
    }

    #[test]
    fn mistakes_are_reported_with_positions() {
        assert_eq!(
            parse("dupliate-line = []\n").unwrap_err(),
            ["line 1, column 1: unknown action dupliate-line; did you mean duplicate-line?"]
        );
        assert_eq!(
            parse("undo = [\"<Control>u\"]\n").unwrap_err(),
            [
                "line 1, column 1: undo keeps the text view's own keys (Ctrl+Z); they can't be \
                 changed"
            ]
        );
        assert_eq!(
            parse("open-recent = []\n").unwrap_err(),
            [
                "line 1, column 1: open-recent takes a parameter from its menu entries and has no keys"
            ]
        );
        assert_eq!(
            parse("save = \"<Control>s\"\n").unwrap_err(),
            ["line 1, column 8: save takes a list of keys, like save = [\"<Control>d\"]"]
        );
        assert_eq!(
            parse("save = [\"<Control>s\", 3]\n").unwrap_err(),
            ["line 1, column 23: a key is a string in quotes, like \"<Control>d\", not integer"]
        );
        assert_eq!(
            parse("save = [\"<Ctrl>ss\"]\n").unwrap_err(),
            [
                "line 1, column 9: “<Ctrl>ss” is not a key; keys look like <Control>d, \
                 <Alt><Shift>Up or F3"
            ]
        );
        assert_eq!(
            parse("save = [\"<Shift>s\"]\n").unwrap_err(),
            [
                "line 1, column 9: “<Shift>s” has no Ctrl, Alt or Super, so it would take the \
                 key from typing (F-keys are the exception)"
            ]
        );
        assert_eq!(
            parse("save = [\"<Control>s\", \"<Ctrl>S\"]\n").unwrap_err(),
            ["line 1, column 23: “<Ctrl>S” is listed twice for save"]
        );
        assert!(parse("save = [\"F9\"]\n").is_ok());
        let syntax = parse("save = [\n").unwrap_err();
        assert!(syntax[0].starts_with("line 1,") || syntax[0].starts_with("line 2,"));
    }

    #[test]
    fn errors_come_in_file_order() {
        let errors = parse("zzz = []\nsave = \"x\"\nyyy = []\n").unwrap_err();
        assert_eq!(errors.len(), 3);
        assert!(errors[0].starts_with("line 1,"));
        assert!(errors[1].starts_with("line 2,"));
        assert!(errors[2].starts_with("line 3,"));
    }

    #[test]
    fn the_default_file_changes_nothing_and_every_line_is_valid() {
        let file = default_keys_file();
        assert_eq!(parse(&file), Ok(Keymap::default()));
        let uncommented: String = file
            .lines()
            .filter_map(|line| line.strip_prefix("# "))
            .filter(|line| !line.starts_with(' ') && line.contains(" = ["))
            .map(|line| format!("{line}\n"))
            .collect();
        let keymap = parse(&uncommented).unwrap();
        let spelled = |keys: Vec<String>| -> Vec<String> {
            keys.iter().filter_map(|key| canonical(key)).collect()
        };
        for id in ActionId::ALL {
            assert_eq!(
                spelled(keymap.installed(id)),
                spelled(Keymap::default().installed(id)),
                "{id:?}"
            );
            assert_eq!(keymap.is_changed(id), is_rebindable(id), "{id:?}");
        }
        assert!(file.contains("# Edit › Line Operations\n"));
        assert!(!file.contains("\n# undo = "));
        assert!(file.starts_with(
            "# Stet keyboard shortcuts (keys.toml): your changes to Stet's default keys.\n"
        ));
    }

    #[test]
    fn the_user_guide_lists_the_registry_keymap() {
        let guide = include_str!("../../../docs/USER_GUIDE.md");
        let start = guide.find(TABLE_START).expect("the table's start marker") + TABLE_START.len();
        let end = guide[start..]
            .find(TABLE_END)
            .expect("the table's end marker")
            + start;
        let table = markdown_table();
        assert_eq!(
            guide[start..end].trim(),
            table.trim(),
            "docs/USER_GUIDE.md's keymap table differs from the registry; replace it with \
             the table above (markdown_table)"
        );
        assert!(table.contains("| Edit › Line Operations | Duplicate Current Line | Ctrl+D | Editor | `duplicate-line` |"));
        assert!(table.contains("| Edit | Undo | Ctrl+Z | Text fields | — |"));
    }
}
