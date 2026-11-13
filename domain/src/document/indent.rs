//! Indentation: a document's tab width and whether Tab inserts spaces, from the settings, the
//! language, or the indentation the text already uses.

use crate::view::TabWidth;

/// How much of a text [`detect`] reads: enough lines to see the indentation of most files,
/// cheap to take from the start of a large one.
pub const DETECT_MAX_LINES: usize = 10_000;
/// The most bytes [`detect`] reads from the start of a text; [`sample`] cuts there.
pub const DETECT_MAX_BYTES: usize = 256 * 1024;

/// A document's indentation settings: its tab width and what Tab inserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indentation {
    /// The columns between tab stops.
    pub tab_width: TabWidth,
    /// Tab inserts spaces up to the next tab stop instead of a tab character.
    pub insert_spaces: bool,
}

impl Indentation {
    /// The default: tabs, four columns wide.
    pub const DEFAULT: Self = Self {
        tab_width: TabWidth::DEFAULT,
        insert_spaces: false,
    };

    /// The status bar's item: `Spaces: 4` or `Tab Width: 4`, as other editors say it.
    #[must_use]
    pub fn label(self) -> String {
        if self.insert_spaces {
            format!("Spaces: {}", self.tab_width)
        } else {
            format!("Tab Width: {}", self.tab_width)
        }
    }

    /// These settings with what a text's own indentation says; a text indented with tabs
    /// keeps the tab width.
    #[must_use]
    pub fn with_detected(self, detected: Detected) -> Self {
        Self {
            tab_width: detected.width.unwrap_or(self.tab_width),
            insert_spaces: detected.insert_spaces,
        }
    }
}

impl Default for Indentation {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What a text's indentation says: spaces or tabs, and for spaces the step between levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    /// Lines are indented with spaces rather than tabs.
    pub insert_spaces: bool,
    /// The step between indentation levels when it is spaces and one step stands out.
    pub width: Option<TabWidth>,
}

/// Steps that can be an indentation width, in the order ties are broken.
const WIDTHS: [u8; 7] = [4, 2, 8, 3, 6, 5, 7];

/// Reads the indentation of the first [`DETECT_MAX_LINES`] lines of `text`: whether more
/// lines start with a tab or with two or more spaces (a single space is alignment, as in the
/// ` * ` of a block comment), and for spaces the most common step between the indentation of
/// neighbouring lines. `None` when no line is indented.
pub fn detect(text: &str) -> Option<Detected> {
    let mut tabs = 0usize;
    let mut spaces = 0usize;
    let mut steps = [0usize; 9];
    let mut previous: Option<usize> = None;
    for line in text.lines().take(DETECT_MAX_LINES) {
        let content = line.trim_start_matches([' ', '\t']);
        if content.is_empty() {
            continue;
        }
        let indent = &line[..line.len() - content.len()];
        if indent.starts_with('\t') {
            tabs += 1;
            previous = None;
            continue;
        }
        let width = indent.bytes().take_while(|&byte| byte == b' ').count();
        if width >= 2 {
            spaces += 1;
        }
        if let Some(previous) = previous {
            let step = width.abs_diff(previous);
            if (2..steps.len()).contains(&step) {
                steps[step] += 1;
            }
        }
        previous = Some(width);
    }
    if tabs == 0 && spaces == 0 {
        return None;
    }
    if tabs > spaces {
        return Some(Detected {
            insert_spaces: false,
            width: None,
        });
    }
    let best = WIDTHS
        .into_iter()
        .filter_map(TabWidth::new)
        .filter(|width| steps[width.get()] > 0)
        .fold(None, |best: Option<TabWidth>, width| match best {
            Some(best) if steps[best.get()] >= steps[width.get()] => Some(best),
            _ => Some(width),
        });
    Some(Detected {
        insert_spaces: true,
        width: best,
    })
}

/// The start of `text` that [`detect`] needs: at most [`DETECT_MAX_BYTES`], cut at a
/// character boundary.
#[must_use]
pub fn sample(text: &str) -> &str {
    let mut end = text.len().min(DETECT_MAX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(cells: u8) -> TabWidth {
        TabWidth::new(cells).unwrap()
    }

    fn spaces(cells: u8) -> Detected {
        Detected {
            insert_spaces: true,
            width: Some(width(cells)),
        }
    }

    const TABS: Option<Detected> = Some(Detected {
        insert_spaces: false,
        width: None,
    });

    #[test]
    fn finds_the_step_between_levels() {
        let python = "def f():\n    if x:\n        return 1\n    return 2\n";
        assert_eq!(detect(python), Some(spaces(4)));
        let yaml = "a:\n  b:\n    c: 1\n  d: 2\ne: 3\n";
        assert_eq!(detect(yaml), Some(spaces(2)));
        let three = "a\n   b\n      c\n";
        assert_eq!(detect(three), Some(spaces(3)));
    }

    #[test]
    fn tabs_win_when_more_lines_start_with_one() {
        let go = "func f() {\n\tif x {\n\t\treturn\n\t}\n}\n";
        assert_eq!(detect(go), TABS);
        let makefile = "all:\n\tcc -o a a.c\n\t@echo done\n";
        assert_eq!(detect(makefile), TABS);
    }

    #[test]
    fn single_spaces_are_alignment() {
        let c = "/*\n * A comment.\n */\nint x;\n";
        assert_eq!(detect(c), None);
        let tabbed_c = "/*\n * A comment.\n */\nvoid f() {\n\tg();\n}\n";
        assert_eq!(detect(tabbed_c), TABS);
    }

    #[test]
    fn blank_lines_and_plain_text_say_nothing() {
        assert_eq!(detect(""), None);
        assert_eq!(detect("one\ntwo\n\n   \nthree\n"), None);
        // Blank lines don't reset the previous level.
        assert_eq!(detect("a:\n\n  b: 1\n\n  c: 2\n"), Some(spaces(2)));
    }

    #[test]
    fn a_dedent_of_two_levels_counts_less_than_single_steps() {
        let rust = "fn f() {\n    if x {\n        y();\n    }\n}\nfn g() {\n    if z {\n        w();\n}\n}\n";
        assert_eq!(detect(rust), Some(spaces(4)));
    }

    #[test]
    fn ties_prefer_four() {
        assert_eq!(detect("a\n    b\n  c\n"), Some(spaces(4)));
        assert_eq!(detect("a\n  b\n        c\n"), Some(spaces(2)));
    }

    #[test]
    fn labels_and_merging() {
        let spaces4 = Indentation {
            tab_width: width(4),
            insert_spaces: true,
        };
        assert_eq!(spaces4.label(), "Spaces: 4");
        assert_eq!(Indentation::DEFAULT.label(), "Tab Width: 4");
        assert_eq!(
            Indentation::DEFAULT.with_detected(spaces(2)),
            Indentation {
                tab_width: width(2),
                insert_spaces: true
            }
        );
        assert_eq!(
            spaces4.with_detected(TABS.unwrap()),
            Indentation {
                tab_width: width(4),
                insert_spaces: false
            }
        );
    }

    #[test]
    fn the_sample_ends_at_a_character_boundary() {
        let text = "ø".repeat(DETECT_MAX_BYTES);
        let sample = sample(&text);
        assert!(sample.len() <= DETECT_MAX_BYTES);
        assert!(text.starts_with(sample));
        assert_eq!(super::sample("short"), "short");
    }
}
