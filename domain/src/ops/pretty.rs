//! Pretty-printing a file whose lines are too long to show: JSON or XML, guessed from the
//! text.

use super::{Indent, SyntaxError, json, xml};

/// A language the long-line path can pretty-print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Json,
    Xml,
}

impl FormatKind {
    /// Guesses from the first character that isn't whitespace.
    pub fn sniff(text: &str) -> Option<Self> {
        match text.trim_start().chars().next()? {
            '{' | '[' => Some(Self::Json),
            '<' => Some(Self::Xml),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Xml => "XML",
        }
    }

    /// The GtkSourceView language id.
    pub const fn language(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }
}

/// Pretty-prints `text`, keeping every token as written. An error says where the text stops
/// being valid, and reads as "expected `,` at line 1, column 42".
pub fn format(text: &str, kind: FormatKind) -> Result<String, SyntaxError> {
    match kind {
        FormatKind::Json => json::format(text, Indent::default()),
        FormatKind::Xml => xml::format(text, Indent::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_json_and_xml() {
        assert_eq!(FormatKind::sniff("  \n{\"a\":1}"), Some(FormatKind::Json));
        assert_eq!(FormatKind::sniff("[1,2]"), Some(FormatKind::Json));
        assert_eq!(
            FormatKind::sniff("<?xml version=\"1.0\"?><a/>"),
            Some(FormatKind::Xml)
        );
        assert_eq!(FormatKind::sniff("plain"), None);
        assert_eq!(FormatKind::sniff(""), None);
    }

    #[test]
    fn formats_or_reports_where_it_failed() {
        assert_eq!(
            format("{\"a\":[1,2]}", FormatKind::Json).unwrap(),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
        let error = format("{\"a\":}", FormatKind::Json).unwrap_err();
        assert_eq!((error.line, error.column, error.offset), (1, 6, 5));
        assert!(
            error.to_string().ends_with(" at line 1, column 6"),
            "{error}"
        );
        assert!(
            format("<a><b/></a>", FormatKind::Xml)
                .unwrap()
                .contains('\n')
        );
    }
}
