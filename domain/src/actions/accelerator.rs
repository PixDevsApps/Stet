//! Accelerator strings: parsing, comparing and showing them, and the editor's built-in
//! bindings that Stet's keymap takes over.

/// An accelerator string split into modifiers and key name, as GTK spells them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Accelerator {
    /// Whether Control (GTK's `<Primary>` on Linux) is held.
    pub control: bool,
    /// Whether Shift is held.
    pub shift: bool,
    /// Whether Alt is held.
    pub alt: bool,
    /// Whether Super is held.
    pub super_key: bool,
    /// The key name, lowercased for letters (GTK lowercases them too).
    pub key: String,
}

impl Accelerator {
    /// Parses `<Control><Shift>z`-style strings. Accepts GTK's modifier spellings; rejects
    /// empty keys and unknown modifiers. Key names are checked by [`is_key_name`].
    #[must_use]
    pub fn parse(accel: &str) -> Option<Self> {
        let mut rest = accel;
        let mut parsed = Self {
            control: false,
            shift: false,
            alt: false,
            super_key: false,
            key: String::new(),
        };
        while let Some(tail) = rest.strip_prefix('<') {
            let (modifier, after) = tail.split_once('>')?;
            match modifier.to_ascii_lowercase().as_str() {
                "control" | "ctrl" | "ctl" | "primary" => parsed.control = true,
                "shift" | "shft" => parsed.shift = true,
                "alt" | "mod1" => parsed.alt = true,
                "super" => parsed.super_key = true,
                _ => return None,
            }
            rest = after;
        }
        if !is_key_name(rest) {
            return None;
        }
        parsed.key = if rest.chars().count() == 1 {
            rest.to_ascii_lowercase()
        } else {
            rest.to_owned()
        };
        Some(parsed)
    }

    /// F1–F35: the keys the user's F-row sends as media keys unless Fn is held.
    #[must_use]
    pub fn is_function_key(&self) -> bool {
        self.key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=35).contains(&number))
    }

    /// One spelling per key, `<Control><Shift><Alt><Super>key`, to compare keys written
    /// differently (`<Ctrl>D`, `<Control>d`).
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.control, "<Control>"),
            (self.shift, "<Shift>"),
            (self.alt, "<Alt>"),
            (self.super_key, "<Super>"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&self.key);
        out
    }

    /// `Ctrl+Shift+Z`, for menus and the palette.
    #[must_use]
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.super_key, "Super+"),
            (self.control, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&display_key(&self.key));
        out
    }
}

fn display_key(key: &str) -> String {
    match key {
        "plus" => "+".to_owned(),
        "minus" => "-".to_owned(),
        "equal" => "=".to_owned(),
        "Page_Up" => "PgUp".to_owned(),
        "Page_Down" => "PgDn".to_owned(),
        "Return" => "Enter".to_owned(),
        "Up" => "↑".to_owned(),
        "Down" => "↓".to_owned(),
        "Left" => "←".to_owned(),
        "Right" => "→".to_owned(),
        "slash" => "/".to_owned(),
        "backslash" => "\\".to_owned(),
        "Delete" => "Del".to_owned(),
        "Insert" => "Ins".to_owned(),
        _ => match key.strip_prefix("KP_") {
            Some("Add") => "Num +".to_owned(),
            Some("Subtract") => "Num -".to_owned(),
            Some("Divide") => "Num /".to_owned(),
            Some(other) => format!("Num {}", display_key(other)),
            None if key.chars().count() == 1 => key.to_ascii_uppercase(),
            None => key.to_owned(),
        },
    }
}

/// Key names the registry may use: letters, digits, F-keys and a fixed list of GDK key names.
/// The app's tests also run every accelerator through GTK's own parser.
#[must_use]
pub fn is_key_name(key: &str) -> bool {
    const NAMED: &[&str] = &[
        "Return",
        "Tab",
        "Escape",
        "Delete",
        "Insert",
        "BackSpace",
        "Home",
        "End",
        "Page_Up",
        "Page_Down",
        "Up",
        "Down",
        "Left",
        "Right",
        "space",
        "plus",
        "minus",
        "equal",
        "period",
        "comma",
        "slash",
        "backslash",
        "semicolon",
        "apostrophe",
        "grave",
        "bracketleft",
        "bracketright",
        "KP_Add",
        "KP_Subtract",
        "KP_Divide",
        "KP_Multiply",
        "KP_Enter",
        "KP_Up",
        "KP_Down",
        "KP_Left",
        "KP_Right",
        "KP_Page_Up",
        "KP_Next",
        "KP_Page_Down",
        "KP_Home",
        "KP_End",
    ];
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.is_ascii_alphanumeric(),
        _ => {
            NAMED.contains(&key)
                || key
                    .strip_prefix('F')
                    .and_then(|number| number.parse::<u8>().ok())
                    .is_some_and(|number| (1..=35).contains(&number))
        }
    }
}

/// GtkSourceView and GtkTextView key bindings that clash with Stet's keymap (ADR-009):
/// `change-number` (and GtkTextView's unselect-all on Ctrl+Shift+A), `move-lines`,
/// `move-words` (back and forward), `move-viewport` (column selection), and GtkTextView's
/// select-all and unselect-all on Ctrl+/ and Ctrl+\. An editor-scoped controller in the
/// capture phase takes these keys before the view, so they do nothing unless the keymap gives
/// them to an action. The keypad variants are GtkSourceView's.
pub const OVERRIDDEN_BUILTINS: &[&str] = &[
    "<Control><Shift>x",
    "<Control><Shift>a",
    "<Alt>Up",
    "<Alt>KP_Up",
    "<Alt>Down",
    "<Alt>KP_Down",
    "<Alt>Left",
    "<Alt>KP_Left",
    "<Alt>Right",
    "<Alt>KP_Right",
    "<Alt><Shift>Up",
    "<Alt><Shift>KP_Up",
    "<Alt><Shift>Down",
    "<Alt><Shift>KP_Down",
    "<Alt><Shift>Page_Up",
    "<Alt><Shift>KP_Page_Up",
    "<Alt><Shift>Page_Down",
    "<Alt><Shift>KP_Next",
    "<Alt><Shift>Home",
    "<Alt><Shift>KP_Home",
    "<Alt><Shift>End",
    "<Alt><Shift>KP_End",
    "<Control>slash",
    "<Control>backslash",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionId;
    use std::collections::HashSet;

    #[test]
    fn overridden_builtins_are_well_formed_and_unique() {
        let mut seen = HashSet::new();
        for key in OVERRIDDEN_BUILTINS {
            let accel = Accelerator::parse(key).unwrap_or_else(|| panic!("{key}"));
            assert!(seen.insert(accel.canonical()), "{key} twice");
        }
        // Copy Current Line takes Ctrl+Shift+X from GtkSourceView's change-number.
        let copy_line = Accelerator::parse(ActionId::CopyLine.keys().app[0]).unwrap();
        assert!(seen.contains(&copy_line.canonical()));
    }

    #[test]
    fn canonical_spellings_compare_keys() {
        let canonical = |key: &str| Accelerator::parse(key).unwrap().canonical();
        assert_eq!(canonical("<Ctrl>D"), "<Control>d");
        assert_eq!(canonical("<Shift><Primary>x"), "<Control><Shift>x");
        assert_eq!(canonical("<Alt><Shift>KP_Up"), "<Shift><Alt>KP_Up");
        assert_eq!(canonical("F3"), "F3");
    }

    #[test]
    fn accelerators_display_readably() {
        let display = |key: &str| Accelerator::parse(key).unwrap().display();
        assert_eq!(display("<Control><Shift>z"), "Ctrl+Shift+Z");
        assert_eq!(display("<Control><Alt>s"), "Ctrl+Alt+S");
        assert_eq!(display("<Alt>Down"), "Alt+↓");
        assert_eq!(display("<Alt><Shift>Left"), "Alt+Shift+←");
        assert_eq!(display("<Alt><Shift>KP_Page_Up"), "Alt+Shift+Num PgUp");
        assert_eq!(display("<Control>plus"), "Ctrl++");
        assert_eq!(display("<Control>KP_Add"), "Ctrl+Num +");
        assert_eq!(display("<Control>Page_Down"), "Ctrl+PgDn");
        assert_eq!(display("<Alt>Return"), "Alt+Enter");
        assert_eq!(display("F11"), "F11");
        assert_eq!(
            Accelerator::parse("<Control>Z"),
            Accelerator::parse("<ctrl>z")
        );
    }
}
