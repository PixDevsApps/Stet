//! How a page shows its document: the window's view settings, the document map, indentation,
//! the language and its highlighting (ADR-014), and the theme's colours.

use super::document::{CURRENT_MATCH_TAG, IndentSource};
use super::page::EditorPage;
use super::search::{OWN_MATCH_TAG, style_tag};
use super::tabstops::TabStops;
use crate::theme::scheme::{BOOKMARK_STYLE, SEARCH_CURRENT_STYLE};
use gtk4 as gtk;
use gtk4::subclass::prelude::*;
use sourceview5::prelude::*;
use stet_domain::document::indent::Indentation;

/// Window-wide view settings, applied to every editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewSettings {
    pub wrap: bool,
    pub whitespace: bool,
    /// The document map (GtkSourceMap) beside the text; never in large-file mode.
    pub map: bool,
}

impl EditorPage {
    pub fn apply_settings(&self, settings: ViewSettings) {
        self.imp().settings.set(settings);
        let large = self.large_file_mode();
        let view = self.view();
        let wrap = settings.wrap && !large && !self.imp().compared.get();
        view.set_wrap_mode(if wrap {
            gtk::WrapMode::WordChar
        } else {
            gtk::WrapMode::None
        });
        let drawer = view.space_drawer();
        drawer.set_types_for_locations(
            sourceview5::SpaceLocationFlags::ALL,
            sourceview5::SpaceTypeFlags::SPACE
                | sourceview5::SpaceTypeFlags::TAB
                | sourceview5::SpaceTypeFlags::NBSP,
        );
        drawer.set_enable_matrix(settings.whitespace && !large);
        self.buffer().set_highlight_matching_brackets(!large);
        self.show_map(settings.map && !large);
    }

    /// Whether the view is in a comparison, which keeps word wrap off.
    pub fn set_compared(&self, compared: bool) {
        self.imp().compared.set(compared);
        self.apply_settings(self.imp().settings.get());
    }

    pub fn is_compared(&self) -> bool {
        self.imp().compared.get()
    }

    /// Shows or hides the document map. Hiding removes it: GtkSourceMap shown again after
    /// being hidden sets its scroll position from an empty allocation, and GTK logs a
    /// CRITICAL ("gtk_adjustment_set_value: assertion 'isfinite (value)' failed").
    fn show_map(&self, show: bool) {
        let imp = self.imp();
        if !show {
            if let Some(map) = imp.map.take() {
                imp.body.get().expect("constructed").remove(&map);
            }
            return;
        }
        let mut map = imp.map.borrow_mut();
        let map = map.get_or_insert_with(|| {
            let map = sourceview5::Map::new();
            map.set_view(self.view());
            map.add_css_class("stet-map");
            imp.body.get().expect("constructed").append(&map);
            map
        });
        map.set_visible(true);
    }

    /// Whether the document map is on screen.
    #[cfg(feature = "selftest")]
    pub fn map_shown(&self) -> bool {
        self.imp()
            .map
            .borrow()
            .as_ref()
            .is_some_and(WidgetExt::is_visible)
    }

    pub fn indentation(&self) -> Indentation {
        self.document().indentation.get()
    }

    pub fn indent_source(&self) -> IndentSource {
        self.document().indent_source.get()
    }

    /// Sets the tab width and what Tab inserts, in every view of the document. The exact tab
    /// stops follow at the next layout (ADR-015, `notify::tab-width`).
    pub fn set_indentation(&self, indentation: Indentation, source: IndentSource) {
        let document = self.document();
        document.indentation.set(indentation);
        document.indent_source.set(source);
        for page in document.live_pages() {
            let view = page.view();
            let tab_width = u32::from(indentation.tab_width);
            if view.tab_width() != tab_width {
                view.set_tab_width(tab_width);
            }
            view.set_insert_spaces_instead_of_tabs(indentation.insert_spaces);
            view.set_indent_width(-1);
        }
    }

    pub fn tab_stops(&self) -> &TabStops {
        &self.imp().tab_stops
    }

    /// Whether syntax highlighting is on for this document (ADR-014): set when it opens,
    /// by size, and by "Highlight Anyway".
    pub fn highlighting(&self) -> bool {
        self.document().highlighting.get()
    }

    /// Switches highlighting on or off. Off detaches the language from the buffer: with a
    /// language GtkSourceView analyses the whole text even when it draws no highlighting,
    /// and that analysis is what holds off frames (ADR-014 amendment).
    pub fn set_highlighting(&self, on: bool) {
        self.document().highlighting.set(on);
        self.sync_language();
    }

    /// Highlighting as a text of its size gets it once loaded: off in large-file mode, and off
    /// when `over_limits` (the settings' highlighting limits), where "Highlight Anyway" can
    /// turn it on.
    pub fn limit_highlighting(&self, over_limits: bool) {
        let large = self.large_file_mode();
        self.document().state.borrow_mut().highlight_limited = over_limits && !large;
        self.set_highlighting(!large && !over_limits);
    }

    /// "Highlight Anyway": highlighting on, despite the limits.
    pub fn highlight_anyway(&self) {
        self.document().state.borrow_mut().highlight_limited = false;
        self.set_highlighting(true);
    }

    /// The document's language: what the status bar shows and the picker sets. The buffer
    /// has it only while highlighting is on.
    pub fn language(&self) -> Option<sourceview5::Language> {
        self.document().language.borrow().clone()
    }

    pub fn set_language(&self, language: Option<&sourceview5::Language>) {
        self.document().language.replace(language.cloned());
        self.sync_language();
    }

    pub(super) fn sync_language(&self) {
        let document = self.document();
        let on = document.highlighting.get() && !document.state.borrow().is_loading();
        let buffer = self.buffer();
        let wanted = if on {
            document.language.borrow().clone()
        } else {
            None
        };
        if buffer.language() != wanted {
            buffer.set_language(wanted.as_ref());
        }
        buffer.set_highlight_syntax(on);
    }

    /// Applies a theme's scheme, including the search, mark and comparison styles (DESIGN.md),
    /// and the bookmark in the gutter.
    pub fn set_scheme(&self, scheme: &sourceview5::StyleScheme) {
        let buffer = self.buffer();
        buffer.set_style_scheme(Some(scheme));
        let table = buffer.tag_table();
        for (tag, style) in [
            (CURRENT_MATCH_TAG, SEARCH_CURRENT_STYLE),
            (OWN_MATCH_TAG, "search-match"),
        ] {
            if let Some(tag) = table.lookup(tag) {
                style_tag(&tag, scheme, style);
            }
        }
        for style in stet_domain::marks::MarkStyle::ALL {
            if let Some(tag) = table.lookup(super::marks::tag_name(style)) {
                style_tag(&tag, scheme, super::marks::scheme_style(style));
            }
        }
        super::comparison::recolor(&buffer, scheme);
        self.document().search.set_scheme(scheme);
        let color = scheme
            .style(BOOKMARK_STYLE)
            .and_then(|style| style.foreground())
            .and_then(|color| gtk::gdk::RGBA::parse(color.as_str()).ok())
            .unwrap_or_else(|| self.view().color());
        if let Some(attributes) = self.imp().bookmark.get() {
            attributes.set_pixbuf(&super::marks::bookmark_pixbuf(&color, 64));
        }
        self.view().queue_draw();
    }
}
