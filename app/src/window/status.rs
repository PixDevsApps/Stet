//! The status bar: Ln/Col/Sel (characters and lines), the document's lines and characters, an
//! optional word count, the language (a button that opens the language picker), the line
//! ending (a button with the line-ending menu), the encoding (a button that opens the
//! encoding picker), the indentation (a button that opens its popover) and INS/OVR; while two
//! documents are compared, the comparison's summary (M7).

use gtk4 as gtk;
use gtk4::gio;
use gtk4::prelude::*;

pub struct StatusBar {
    pub root: gtk::Box,
    position: gtk::Label,
    length: gtk::Label,
    words: gtk::Label,
    words_separator: gtk::Separator,
    compare: gtk::Label,
    compare_separator: gtk::Separator,
    pub language: gtk::MenuButton,
    pub eol: gtk::MenuButton,
    pub encoding: gtk::MenuButton,
    pub indentation: gtk::MenuButton,
    overwrite: gtk::Label,
}

impl StatusBar {
    pub fn new(
        language_picker: &gtk::Popover,
        eol_menu: &gio::MenuModel,
        encoding_picker: &gtk::Popover,
        indentation_picker: &gtk::Popover,
    ) -> Self {
        let label = |text: &str, tooltip: &str| {
            gtk::Label::builder()
                .label(text)
                .tooltip_text(tooltip)
                .xalign(0.0)
                .build()
        };
        let menu_button = |text: &str, tooltip: &str| {
            let button = gtk::MenuButton::builder()
                .label(text)
                .tooltip_text(tooltip)
                .always_show_arrow(false)
                .build();
            button.add_css_class("flat");
            button
        };
        let position = label(
            "Ln 1, Col 1, Sel 0",
            "Line, column, and the selected characters | lines",
        );
        let length = label("1 lines, 0 chars", "Lines and characters in the document");
        let words = label("0 words", "Words in the document");
        words.set_visible(false);
        let words_separator = gtk::Separator::new(gtk::Orientation::Vertical);
        words_separator.set_visible(false);
        let compare = label(
            "",
            "The comparison: lines added, removed, changed and moved",
        );
        compare.set_visible(false);
        let compare_separator = gtk::Separator::new(gtk::Orientation::Vertical);
        compare_separator.set_visible(false);
        let language = menu_button("Plain Text", "Syntax language");
        language.set_popover(Some(language_picker));
        let eol = menu_button("LF", "Line ending the next save writes");
        eol.set_menu_model(Some(eol_menu));
        let encoding = menu_button("UTF-8", "Encoding: reinterpret the file, or convert it");
        encoding.set_popover(Some(encoding_picker));
        let indentation = menu_button("Tab Width: 4", "Indentation: spaces or tabs, and width");
        indentation.set_popover(Some(indentation_picker));
        let overwrite = label("INS", "Insert or overwrite (Insert key)");
        let spacer = gtk::Box::builder().hexpand(true).build();
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("stet-statusbar");
        root.add_css_class("stet-mono");
        root.append(&position);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        root.append(&length);
        root.append(&words_separator);
        root.append(&words);
        root.append(&compare_separator);
        root.append(&compare);
        root.append(&spacer);
        for (index, widget) in [
            language.upcast_ref::<gtk::Widget>(),
            eol.upcast_ref(),
            encoding.upcast_ref(),
            indentation.upcast_ref(),
            overwrite.upcast_ref(),
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 {
                root.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            }
            root.append(widget);
        }
        Self {
            root,
            position,
            length,
            words,
            words_separator,
            compare,
            compare_separator,
            language,
            eol,
            encoding,
            indentation,
            overwrite,
        }
    }

    /// 1-based line and column; `selected` counts characters, `lines` the lines the
    /// selection touches (`Sel 12 | 2`).
    pub fn set_position(&self, line: usize, column: usize, selected: usize, lines: usize) {
        let label = if selected == 0 {
            format!("Ln {line}, Col {column}, Sel 0")
        } else {
            format!(
                "Ln {line}, Col {column}, Sel {} | {}",
                grouped(selected),
                grouped(lines)
            )
        };
        self.position.set_label(&label);
    }

    /// The document's size: `3 lines, 38 chars`.
    pub fn set_length(&self, lines: usize, chars: usize) {
        self.length.set_label(&format!(
            "{} {}, {} chars",
            grouped(lines),
            if lines == 1 { "line" } else { "lines" },
            grouped(chars)
        ));
    }

    /// The word count, when config.toml's `word_count` is on; `None` while it is counted.
    pub fn set_words(&self, words: Option<usize>) {
        self.words.set_label(&match words {
            Some(1) => "1 word".to_owned(),
            Some(words) => format!("{} words", grouped(words)),
            None => "… words".to_owned(),
        });
    }

    pub fn show_words(&self, show: bool) {
        self.words.set_visible(show);
        self.words_separator.set_visible(show);
    }

    pub fn words_shown(&self) -> bool {
        self.words.is_visible()
    }

    /// The comparison's summary, or nothing when there is none (M7).
    pub fn set_compare(&self, summary: Option<&str>) {
        self.compare.set_label(summary.unwrap_or_default());
        self.compare.set_visible(summary.is_some());
        self.compare_separator.set_visible(summary.is_some());
    }

    /// `Spaces: 4` or `Tab Width: 4`.
    pub fn set_indentation(&self, label: &str) {
        self.indentation.set_label(label);
    }

    /// Column mode (M6): the cursor corner, virtual columns included, and the rectangle's
    /// rows × columns (`Sel 3×4`).
    pub fn set_column_position(&self, line: usize, column: usize, rows: usize, columns: usize) {
        self.position
            .set_label(&format!("Ln {line}, Col {column}, Sel {rows}×{columns}"));
    }

    pub fn set_language(&self, name: &str) {
        self.language.set_label(name);
    }

    /// `LF`, `CRLF`, `CR` or `Mixed`.
    pub fn set_eol(&self, label: &str) {
        self.eol.set_label(label);
    }

    /// `UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM`, `Windows-1252`…
    pub fn set_encoding(&self, label: &str) {
        self.encoding.set_label(label);
    }

    pub fn set_overwrite(&self, overwrite: bool) {
        self.overwrite
            .set_label(if overwrite { "OVR" } else { "INS" });
    }

    /// Every item's text, for the self-test.
    pub fn text(&self) -> String {
        let button = |button: &gtk::MenuButton| {
            button
                .label()
                .map(|label| label.to_string())
                .unwrap_or_default()
        };
        let mut items = vec![
            self.position.label().to_string(),
            self.length.label().to_string(),
        ];
        if self.words.is_visible() {
            items.push(self.words.label().to_string());
        }
        if self.compare.is_visible() {
            items.push(self.compare.label().to_string());
        }
        items.extend([
            button(&self.language),
            button(&self.eol),
            button(&self.encoding),
            button(&self.indentation),
            self.overwrite.label().to_string(),
        ]);
        items.join(" | ")
    }
}

/// `12345` as `12,345`.
fn grouped(number: usize) -> String {
    super::dialogs::grouped(number)
}
