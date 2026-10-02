//! Deterministic generated inputs for the self-tests (`write-generated`): logs, Rust-like
//! code, single-line JSON and XML, JSON with one object per line, and numbers with blanks
//! around them, of at least a given size.

use std::fmt::Write as _;

/// `kind` text of at least `bytes` bytes; logs and code end with a line break.
pub fn generate(kind: &str, bytes: usize) -> Result<String, String> {
    let mut out = String::with_capacity(bytes + 256);
    match kind {
        "log" => {
            let mut index = 0u64;
            while out.len() < bytes {
                let _ = writeln!(
                    out,
                    "2026-10-01T12:{:02}:{:02}.{:03}Z INFO  [worker-{:02}] GET /api/v1/items/{} \
                     status=200 duration_ms={}.{} user=user{}",
                    index / 60_000 % 60,
                    index / 1000 % 60,
                    index % 1000,
                    index % 16,
                    index * 7 % 100_000,
                    index % 900,
                    index % 10,
                    index % 50
                );
                index += 1;
            }
        }
        "rust" => {
            let mut index = 0u64;
            while out.len() < bytes {
                let _ = write!(
                    out,
                    "/// Item number {index}.\n#[derive(Debug, Clone, Default)]\npub struct \
                     Item{index} {{\n    pub id: u64,\n    pub name: String,\n}}\n\nimpl \
                     Item{index} {{\n    pub fn total(&self, values: &[u64]) -> u64 {{\n        \
                     // Sum the values and add the id.\n        values.iter().sum::<u64>() + \
                     self.id * {index}\n    }}\n}}\n\n"
                );
                index += 1;
            }
        }
        "json-line" | "json-line-unclosed" => {
            out.push_str("{\"version\":1,\"items\":[");
            let mut index = 0u64;
            while out.len() < bytes {
                if index > 0 {
                    out.push(',');
                }
                let _ = write!(
                    out,
                    "{{\"id\":{index},\"name\":\"item {index}\",\"active\":{},\"tags\":[\"a\",\
                     \"b\",\"c\"],\"score\":{index}.5,\"parent\":null}}",
                    index.is_multiple_of(3)
                );
                index += 1;
            }
            if kind == "json-line" {
                out.push_str("]}");
            }
        }
        "json-lines" => {
            out.push_str("[\n");
            let mut index = 0u64;
            while out.len() < bytes {
                if index > 0 {
                    out.push_str(",\n");
                }
                let _ = write!(
                    out,
                    "{{\"id\":{index},\"name\":\"item {index}\",\"tags\":[\"a\",\"b\"],\
                     \"score\":{index}.5}}"
                );
                index += 1;
            }
            out.push_str("\n]\n");
        }
        "numbers" => {
            let mut index = 0u64;
            while out.len() < bytes {
                let _ = writeln!(out, "  {}  ", index * 7919 % 1_000_003);
                index += 1;
            }
        }
        "xml-line" => {
            out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?><items>");
            let mut index = 0u64;
            while out.len() < bytes {
                let _ = write!(
                    out,
                    "<item id=\"{index}\"><name>item {index}</name><tags><tag>a</tag><tag>b\
                     </tag></tags></item>"
                );
                index += 1;
            }
            out.push_str("</items>");
        }
        // Two versions of a text for Compare (M7): `lines-new` is `lines-old` with lines
        // changed, removed and added, and a block of six lines moved further down.
        "lines-old" | "lines-new" => {
            let old = numbered_lines(bytes);
            let lines = if kind == "lines-old" {
                old
            } else {
                edited_lines(&old)
            };
            for line in lines {
                out.push_str(&line);
                out.push('\n');
            }
        }
        other => {
            return Err(format!(
                "unknown kind {other}; use log, rust, json-line, json-line-unclosed, json-lines, \
                 numbers, xml-line, lines-old or lines-new"
            ));
        }
    }
    Ok(out)
}

/// Numbered lines of at least `bytes` bytes in all.
fn numbered_lines(bytes: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut total = 0;
    while total < bytes {
        let index = lines.len();
        let line = format!(
            "line {index:05} alpha beta gamma delta {}",
            index * 7 % 1000
        );
        total += line.len() + 1;
        lines.push(line);
    }
    lines
}

/// `old` with every 50th line changed, every 97th removed, a line added after every 113th, and
/// lines 200 to 205 moved after line 900.
fn edited_lines(old: &[String]) -> Vec<String> {
    let moved = 200..206;
    let mut lines = Vec::with_capacity(old.len() + old.len() / 100);
    for (index, line) in old.iter().enumerate() {
        if moved.contains(&index) && old.len() > 900 {
            continue;
        }
        if index % 97 != 13 {
            if index % 50 == 7 {
                lines.push(line.replace("beta", "BETA"));
            } else {
                lines.push(line.clone());
            }
        }
        if index % 113 == 29 {
            lines.push(format!("inserted after {index:05}"));
        }
        if index == 900 {
            lines.extend(old[moved.clone()].iter().cloned());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_the_kinds() {
        for kind in [
            "log",
            "rust",
            "json-line",
            "xml-line",
            "json-lines",
            "numbers",
            "lines-old",
        ] {
            let text = generate(kind, 10_000).unwrap();
            assert!(text.len() >= 10_000, "{kind}");
            assert_eq!(generate(kind, 10_000).unwrap(), text, "{kind}");
        }
        assert!(!generate("json-line", 100_000).unwrap().contains('\n'));
        assert!(generate("log", 1000).unwrap().ends_with('\n'));
        assert!(stet_domain::ops::json::validate(&generate("json-line", 5000).unwrap()).is_ok());
        assert!(stet_domain::ops::xml::validate(&generate("xml-line", 5000).unwrap()).is_ok());
        assert!(stet_domain::ops::json::validate(&generate("json-lines", 5000).unwrap()).is_ok());
        assert!(generate("numbers", 100).unwrap().starts_with("  0  \n"));
        let (old, new) = (
            generate("lines-old", 50_000).unwrap(),
            generate("lines-new", 50_000).unwrap(),
        );
        let (old_lines, new_lines) = (old.lines().count(), new.lines().count());
        assert!(old_lines > 1000 && new_lines > 1000);
        assert!(new.contains("line 00007 alpha BETA") && !new.contains("line 00013 "));
        assert!(new.contains("inserted after 00029"));
        let moved = new.find("line 00200 ").unwrap();
        assert!(moved > new.find("line 00900 ").unwrap());
        assert!(generate("csv", 10).is_err());
    }
}
