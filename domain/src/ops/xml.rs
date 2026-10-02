//! XML tools. `format` re-indents the markup and copies every token verbatim: tags with the
//! order, quoting and spacing of their attributes, text, comments, CDATA sections, processing
//! instructions, the declaration and the doctype. Only whitespace-only text between elements
//! changes. An element with text in it (mixed content), with nothing but whitespace in it, or
//! under `xml:space="preserve"` is copied as it is. `format` needs complete markup and
//! matching tags and accepts fragments with several roots; `validate` checks
//! well-formedness.

use std::ops::Range;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use super::{Breaks, Change, Indent, SyntaxError, Target, reformat_target};
use crate::text::{LineIndex, Position};

pub type XmlError = SyntaxError;

/// Checks well-formedness, namespaces included, with roxmltree. Internal DTD entities are
/// expanded; a document that uses entities from an external DTD fails.
pub fn validate(text: &str) -> Result<(), XmlError> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    roxmltree::Document::parse_with_options(text, options)
        .map(|_| ())
        .map_err(|error| validation_error(text, &error))
}

/// Pretty-prints `text`, one element, comment or processing instruction per line.
pub fn format(text: &str, indent: Indent) -> Result<String, XmlError> {
    let document = tokenize(text)?;
    Ok(write(text, &document, Breaks::new(indent)))
}

/// [`format()`] on the selection, or on the whole document for a caret or `Document`. The
/// whitespace around the XML stays; error positions are positions in `text`.
pub fn format_target(text: &str, target: Target, indent: Indent) -> Result<Change, XmlError> {
    reformat_target(text, target, |xml| format(xml, indent))
}

fn validation_error(text: &str, error: &roxmltree::Error) -> XmlError {
    let position = error.pos();
    let message = error.to_string().replace(&format!(" at {position}"), "");
    let index = LineIndex::new(text);
    let offset = match error {
        roxmltree::Error::NoRootNode
        | roxmltree::Error::UnclosedRootNode
        | roxmltree::Error::UnexpectedEndOfStream => index.len_chars(),
        _ => index.offset(Position::new(
            (position.row as usize).saturating_sub(1),
            (position.col as usize).saturating_sub(1),
        )),
    };
    let Position { line, column } = index.position(offset);
    SyntaxError {
        message,
        line: line + 1,
        column: column + 1,
        offset,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A start tag, with the index of its end tag and whether the element is copied as it is.
    Start {
        end: usize,
        verbatim: bool,
    },
    End,
    /// Self-closing tags, comments, processing instructions, the declaration and the doctype.
    Markup,
    /// Whitespace-only text.
    Space,
    /// Other text, CDATA sections and references.
    Text,
}

#[derive(Debug)]
struct Token {
    kind: Kind,
    /// Bytes of the source.
    span: Range<usize>,
}

struct Document {
    tokens: Vec<Token>,
    /// Text outside every element, as in a fragment like `a <b/> c`.
    loose_text: bool,
}

struct Open {
    token: usize,
    text: bool,
    markup: bool,
    preserve: bool,
}

fn tokenize(text: &str) -> Result<Document, XmlError> {
    let mut reader = Reader::from_str(text);
    let mut tokens: Vec<Token> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    let mut loose_text = false;
    loop {
        let start = byte_offset(reader.buffer_position());
        let event = reader.read_event().map_err(|error| {
            SyntaxError::at_byte(
                text,
                byte_offset(reader.error_position()),
                error.to_string(),
            )
        })?;
        let span = start..byte_offset(reader.buffer_position());
        let kind = match event {
            Event::Eof => break,
            Event::Start(tag) => {
                let inherited = open.last().is_some_and(|parent| parent.preserve);
                let preserve = match tag.try_get_attribute("xml:space") {
                    Ok(Some(attribute)) => match attribute.value.as_ref() {
                        "preserve" => true,
                        "default" => false,
                        _ => inherited,
                    },
                    Ok(None) => inherited,
                    Err(error) => {
                        return Err(SyntaxError::at_byte(text, span.start, error.to_string()));
                    }
                };
                if let Some(parent) = open.last_mut() {
                    parent.markup = true;
                }
                open.push(Open {
                    token: tokens.len(),
                    text: false,
                    markup: false,
                    preserve,
                });
                Kind::Start {
                    end: 0,
                    verbatim: false,
                }
            }
            Event::End(_) => {
                let Some(element) = open.pop() else {
                    return Err(SyntaxError::at_byte(text, span.start, "unmatched end tag"));
                };
                let verbatim = element.text || element.preserve || !element.markup;
                tokens[element.token].kind = Kind::Start {
                    end: tokens.len(),
                    verbatim,
                };
                Kind::End
            }
            Event::Text(content) if content.bytes().all(|byte| byte.is_ascii_whitespace()) => {
                Kind::Space
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) => {
                match open.last_mut() {
                    Some(parent) => parent.text = true,
                    None => loose_text = true,
                }
                Kind::Text
            }
            Event::Empty(_)
            | Event::Comment(_)
            | Event::PI(_)
            | Event::Decl(_)
            | Event::DocType(_) => {
                if let Some(parent) = open.last_mut() {
                    parent.markup = true;
                }
                Kind::Markup
            }
        };
        tokens.push(Token { kind, span });
    }
    if let Some(element) = open.last() {
        let span = &tokens[element.token].span;
        let name = text[span.start + 1..span.end - 1]
            .split(|c: char| c.is_ascii_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        return Err(SyntaxError::at_byte(
            text,
            span.start,
            format!("`<{name}>` is not closed"),
        ));
    }
    Ok(Document { tokens, loose_text })
}

fn byte_offset(position: u64) -> usize {
    usize::try_from(position).unwrap_or(usize::MAX)
}

fn write(text: &str, document: &Document, mut breaks: Breaks) -> String {
    if document.loose_text {
        return text.to_owned();
    }
    let tokens = &document.tokens;
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    if let Some(first) = tokens.first() {
        out.push_str(&text[..first.span.start]);
    }
    let mut depth = 0usize;
    let mut first = true;
    let mut index = 0;
    while let Some(token) = tokens.get(index) {
        let mut next = index + 1;
        let span = match token.kind {
            Kind::Space => {
                index = next;
                continue;
            }
            Kind::Start {
                end,
                verbatim: true,
            } => {
                next = end + 1;
                token.span.start..tokens[end].span.end
            }
            Kind::End => {
                depth = depth.saturating_sub(1);
                token.span.clone()
            }
            Kind::Start { .. } | Kind::Markup | Kind::Text => token.span.clone(),
        };
        if !first {
            breaks.push(&mut out, depth);
        }
        first = false;
        out.push_str(&text[span]);
        if matches!(
            token.kind,
            Kind::Start {
                verbatim: false,
                ..
            }
        ) {
            depth += 1;
        }
        index = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::apply;
    use proptest::prelude::*;

    fn pretty(text: &str) -> String {
        format(text, Indent::Spaces(2)).unwrap()
    }

    #[test]
    fn formats_nested_elements_and_keeps_every_token() {
        let text = concat!(
            r#"<?xml version="1.0" encoding="UTF-8"?><!-- top --><root a="1" b='x &amp; y'>"#,
            r#"<child>text</child><empty/><e></e><list><item>1</item><item>2</item></list></root>"#
        );
        let expected = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- top -->
<root a="1" b='x &amp; y'>
  <child>text</child>
  <empty/>
  <e></e>
  <list>
    <item>1</item>
    <item>2</item>
  </list>
</root>"#;
        assert_eq!(pretty(text), expected);
        assert_eq!(pretty(expected), expected);
        assert_eq!(
            format("<a><b/></a>", Indent::Tab).unwrap(),
            "<a>\n\t<b/>\n</a>"
        );
    }

    #[test]
    fn reindents_existing_layout() {
        let text = "<a>\n\n      <b/>\n   <c>x</c>\n\t</a>\n";
        assert_eq!(pretty(text), "<a>\n  <b/>\n  <c>x</c>\n</a>");
    }

    #[test]
    fn mixed_content_and_whitespace_content_are_copied() {
        let text = "<doc><p>Hello <b>world</b>!\n  <i>x</i></p><s>   </s><t>\n</t></doc>";
        assert_eq!(
            pretty(text),
            "<doc>\n  <p>Hello <b>world</b>!\n  <i>x</i></p>\n  <s>   </s>\n  <t>\n</t>\n</doc>"
        );
    }

    #[test]
    fn xml_space_preserve_is_copied_with_its_descendants() {
        let text = r#"<doc><pre xml:space="preserve"><a>  <b/></a>
</pre><x xml:space="preserve"><y xml:space="default"><z/></y></x></doc>"#;
        assert_eq!(
            pretty(text),
            "<doc>\n  <pre xml:space=\"preserve\"><a>  <b/></a>\n</pre>\n  <x xml:space=\"preserve\"><y xml:space=\"default\"><z/></y></x>\n</doc>"
        );
    }

    #[test]
    fn keeps_doctype_cdata_processing_instructions_and_references() {
        let text = concat!(
            "<!DOCTYPE note [<!ENTITY x \"y\">]><note><?pi data?><to>&x;&#65;</to>",
            "<code><![CDATA[<raw> & ]]></code><!-- c --></note>"
        );
        assert_eq!(
            pretty(text),
            concat!(
                "<!DOCTYPE note [<!ENTITY x \"y\">]>\n<note>\n  <?pi data?>\n  <to>&x;&#65;</to>\n",
                "  <code><![CDATA[<raw> & ]]></code>\n  <!-- c -->\n</note>"
            )
        );
    }

    #[test]
    fn keeps_tags_verbatim_including_line_breaks_inside_them() {
        let text = "<a\n   x=\"1\"\n   y='2'><b  z = \"3\" /></a >";
        assert_eq!(
            pretty(text),
            "<a\n   x=\"1\"\n   y='2'>\n  <b  z = \"3\" />\n</a >"
        );
    }

    #[test]
    fn formats_fragments_and_leaves_loose_text_alone() {
        assert_eq!(pretty("<a/> <b><c/></b>"), "<a/>\n<b>\n  <c/>\n</b>");
        assert_eq!(pretty("hello <b/> world"), "hello <b/> world");
    }

    #[test]
    fn format_reports_broken_structure() {
        let error = format("<a><b></a>", Indent::default()).unwrap_err();
        assert_eq!((error.line, error.column), (1, 7));
        assert!(
            error.message.contains("expected `</b>`"),
            "{}",
            error.message
        );
        let error = format("<a>\n  <b></b>", Indent::default()).unwrap_err();
        assert_eq!(error.message, "`<a>` is not closed");
        assert_eq!((error.line, error.column, error.offset), (1, 1, 0));
        let error = format("<a><!-- x</a>", Indent::default()).unwrap_err();
        assert_eq!((error.line, error.column), (1, 4));
        assert!(format("</a>", Indent::default()).is_err());
        assert!(format("<a x=\"1></a>", Indent::default()).is_err());
    }

    #[test]
    fn validate_checks_well_formedness() {
        assert_eq!(
            validate("<?xml version=\"1.0\"?>\n<a x='1'><b/>t</a>\n"),
            Ok(())
        );
        assert_eq!(
            validate("<!DOCTYPE a [<!ENTITY e \"v\">]><a>&e;</a>"),
            Ok(())
        );
        let error = validate("<a>\n  <b></a>").unwrap_err();
        assert_eq!(error.message, "expected 'b' tag, not 'a'");
        assert_eq!((error.line, error.column, error.offset), (2, 6, 9));
        let error = validate("<a x='1' x='2'/>").unwrap_err();
        assert_eq!(error.message, "attribute 'x' is already defined");
        let error = validate("<a>&nope;</a>").unwrap_err();
        assert_eq!(error.message, "unknown entity reference 'nope'");
        let error = validate("<p:a/>").unwrap_err();
        assert_eq!(error.message, "an unknown namespace prefix 'p'");
        let error = validate("<a>\n<b>").unwrap_err();
        assert_eq!(error.message, "the root node was opened but never closed");
        assert_eq!((error.line, error.column, error.offset), (2, 4, 7));
        assert!(validate("<a/><b/>").is_err());
        assert!(validate("").is_err());
        assert!(validate("<a>--<!-- x -- y --></a>").is_err());
    }

    #[test]
    fn targets_keep_the_surrounding_whitespace() {
        let text = "\n<a><b/></a>\n";
        let change = format_target(text, Target::Document, Indent::Spaces(4)).unwrap();
        assert_eq!(
            apply(text, change.edits).unwrap(),
            "\n<a>\n    <b/>\n</a>\n"
        );
        let text = "x\n<a><b></a>";
        let error = format_target(text, Target::Selection(2..12), Indent::default()).unwrap_err();
        assert_eq!((error.line, error.column, error.offset), (2, 7, 8));
    }

    fn attributes() -> impl Strategy<Value = String> {
        prop::sample::subsequence(vec!["x", "y", "z"], 0..=3).prop_flat_map(|names| {
            let count = names.len();
            (
                Just(names),
                prop::collection::vec((any::<bool>(), "[a-z0-9 ]{0,3}"), count),
            )
                .prop_map(|(names, values)| {
                    names
                        .iter()
                        .zip(values)
                        .map(|(name, (single, value))| {
                            if single {
                                std::format!(" {name}='{value}'")
                            } else {
                                std::format!(" {name}=\"{value}\"")
                            }
                        })
                        .collect::<String>()
                })
        })
    }

    fn leaf() -> impl Strategy<Value = String> {
        prop_oneof![
            4 => "[a-z]{1,4}( [a-z]{1,4})?",
            2 => "[ \n\t]{1,3}",
            1 => Just("&amp;".to_owned()),
            1 => "<!--[a-z ]{0,5}-->",
            1 => "<!\\[CDATA\\[[a-z<>& ]{0,5}\\]\\]>",
            1 => "<\\?pi( [a-z]{1,3})?\\?>",
            1 => ("[a-c]", attributes()).prop_map(|(name, attributes)| std::format!("<{name}{attributes}/>")),
        ]
    }

    /// A well-formed element with random whitespace, comments, CDATA and mixed content.
    fn element() -> impl Strategy<Value = String> {
        leaf().prop_recursive(4, 40, 5, |inner| {
            ("[a-c]", attributes(), prop::collection::vec(inner, 0..5)).prop_map(
                |(name, attributes, children)| {
                    std::format!("<{name}{attributes}>{}</{name}>", children.concat())
                },
            )
        })
    }

    fn document() -> impl Strategy<Value = String> {
        (any::<bool>(), "[ \n]{0,2}", element(), "[ \n]{0,2}").prop_map(
            |(declaration, before, element, after)| {
                let prolog = if declaration {
                    "<?xml version=\"1.0\"?>\n"
                } else {
                    ""
                };
                std::format!("{prolog}{before}<root>{element}</root>{after}")
            },
        )
    }

    /// The source text of every token except whitespace-only text.
    fn tokens(text: &str) -> Vec<String> {
        tokenize(text)
            .unwrap()
            .tokens
            .iter()
            .filter(|token| token.kind != Kind::Space)
            .map(|token| text[token.span.clone()].to_owned())
            .collect()
    }

    /// The tree without whitespace-only text nodes, as roxmltree sees it.
    fn tree(text: &str) -> Vec<String> {
        let document = roxmltree::Document::parse(text).unwrap();
        document
            .descendants()
            .filter_map(|node| match node.node_type() {
                roxmltree::NodeType::Root => Some("root".to_owned()),
                roxmltree::NodeType::Element => {
                    let attributes: Vec<String> = node
                        .attributes()
                        .map(|a| std::format!("{}={}", a.name(), a.value()))
                        .collect();
                    Some(std::format!("element {:?} {attributes:?}", node.tag_name()))
                }
                roxmltree::NodeType::Text => {
                    let text = node.text().unwrap_or_default();
                    (!text.trim().is_empty()).then(|| std::format!("text {text:?}"))
                }
                roxmltree::NodeType::Comment => Some(std::format!("comment {:?}", node.text())),
                roxmltree::NodeType::PI => Some(std::format!("pi {:?}", node.pi())),
            })
            .collect()
    }

    proptest! {
        #[test]
        fn tokens_cover_the_input_exactly(text in document()) {
            let document = tokenize(&text).unwrap();
            let joined: String = document.tokens.iter().map(|t| &text[t.span.clone()]).collect();
            prop_assert_eq!(joined, text);
        }

        #[test]
        fn format_keeps_tokens_and_the_tree(text in document(), width in 0usize..4) {
            prop_assert_eq!(validate(&text), Ok(()));
            let formatted = format(&text, Indent::Spaces(width)).unwrap();
            prop_assert_eq!(validate(&formatted), Ok(()));
            prop_assert_eq!(tokens(&formatted), tokens(&text));
            prop_assert_eq!(tree(&formatted), tree(&text));
            prop_assert_eq!(format(&formatted, Indent::Spaces(width)).unwrap(), formatted);
        }
    }

    fn sample(bytes: usize) -> String {
        let mut text = String::from("<?xml version=\"1.0\"?>\n<catalog>");
        let mut id = 0u64;
        while text.len() < bytes {
            text.push_str(&std::format!(
                concat!(
                    "<book id=\"{id}\" lang='en'><title>Title {id}</title><author>A &amp; B</author>",
                    "<price currency=\"EUR\">{price}.50</price><!-- note {id} --><tags><tag>x</tag><tag>y</tag></tags>",
                    "<desc>Some <b>mixed</b> text</desc><cover/></book>"
                ),
                id = id,
                price = id % 100
            ));
            id += 1;
        }
        text.push_str("</catalog>\n");
        text
    }

    #[test]
    #[ignore = "timing; run with --release -- --ignored --nocapture"]
    fn timing_on_5_mb() {
        use std::time::Instant;
        let compact = sample(5 * 1024 * 1024);
        let time = |label: &str, run: &dyn Fn() -> String| {
            let mut best = f64::MAX;
            let mut output = String::new();
            for _ in 0..5 {
                let start = Instant::now();
                output = run();
                best = best.min(start.elapsed().as_secs_f64() * 1000.0);
            }
            println!(
                "{label}: best of 5 {best:.1} ms, output {:.1} MB",
                output.len() as f64 / 1_048_576.0
            );
            output
        };
        println!(
            "input: {:.1} MB compact XML",
            compact.len() as f64 / 1_048_576.0
        );
        let formatted = time("format (compact input)", &|| pretty(&compact));
        time("format (formatted input)", &|| pretty(&formatted));
        time("validate", &|| {
            validate(&compact).unwrap();
            String::new()
        });
        time("format_target (Document, with the edit)", &|| {
            let change = format_target(&compact, Target::Document, Indent::Spaces(2)).unwrap();
            assert_eq!(change.edits.len(), 1);
            String::new()
        });
    }
}
