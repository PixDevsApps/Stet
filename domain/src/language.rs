//! Syntax-language hints for what GtkSourceView's own `guess_language` misses: file names it
//! has no glob for, and scripts recognised only by their first line (a shebang, an XML
//! declaration or an editor modeline). Results are GtkSourceView language ids; the caller
//! checks that the id is installed.

const FILE_NAMES: &[(&str, &str)] = &[
    ("PKGBUILD", "sh"),
    (".SRCINFO", "ini"),
    ("Cargo.lock", "toml"),
    ("uv.lock", "toml"),
    ("poetry.lock", "toml"),
    (".bashrc", "sh"),
    (".bash_profile", "sh"),
    (".bash_logout", "sh"),
    (".profile", "sh"),
    (".zshrc", "sh"),
    (".zprofile", "sh"),
    (".zshenv", "sh"),
    (".envrc", "sh"),
    (".gitconfig", "ini"),
    (".editorconfig", "ini"),
    ("Justfile", "makefile"),
    ("justfile", "makefile"),
];

const EXTENSIONS: &[(&str, &str)] = &[
    ("jsonc", "json"),
    ("json5", "json"),
    ("ndjson", "json"),
    ("jsonl", "json"),
    ("install", "sh"),
    ("zsh", "sh"),
    ("bats", "sh"),
    ("service", "ini"),
    ("socket", "ini"),
    ("timer", "ini"),
    ("mount", "ini"),
    ("target", "ini"),
    ("slice", "ini"),
    ("hook", "ini"),
];

/// Interpreter names (with any version suffix removed) and their languages.
const INTERPRETERS: &[(&str, &str)] = &[
    ("sh", "sh"),
    ("bash", "sh"),
    ("dash", "sh"),
    ("ksh", "sh"),
    ("zsh", "sh"),
    ("ash", "sh"),
    ("mksh", "sh"),
    ("fish", "fish"),
    ("python", "python3"),
    ("pypy", "python3"),
    ("perl", "perl"),
    ("ruby", "ruby"),
    ("node", "js"),
    ("nodejs", "js"),
    ("deno", "js"),
    ("bun", "js"),
    ("php", "php"),
    ("lua", "lua"),
    ("luajit", "lua"),
    ("tclsh", "tcl"),
    ("wish", "tcl"),
    ("awk", "awk"),
    ("gawk", "awk"),
    ("mawk", "awk"),
    ("nawk", "awk"),
    ("make", "makefile"),
    ("Rscript", "r"),
    ("julia", "julia"),
    ("rust-script", "rust"),
];

/// Emacs `mode:` and Vim `ft=` values and their languages.
const MODELINES: &[(&str, &str)] = &[
    ("python", "python3"),
    ("sh", "sh"),
    ("shell-script", "sh"),
    ("bash", "sh"),
    ("zsh", "sh"),
    ("rust", "rust"),
    ("c", "c"),
    ("c++", "cpp"),
    ("cpp", "cpp"),
    ("js", "js"),
    ("javascript", "js"),
    ("ts", "typescript"),
    ("typescript", "typescript"),
    ("ruby", "ruby"),
    ("perl", "perl"),
    ("cperl", "perl"),
    ("lua", "lua"),
    ("make", "makefile"),
    ("makefile", "makefile"),
    ("markdown", "markdown"),
    ("md", "markdown"),
    ("json", "json"),
    ("yaml", "yaml"),
    ("toml", "toml"),
    ("xml", "xml"),
    ("nxml", "xml"),
    ("html", "html"),
    ("css", "css"),
    ("sql", "sql"),
    ("ini", "ini"),
    ("conf", "ini"),
    ("dosini", "ini"),
];

/// Every language id a hint can return, for checking them against the installed languages.
pub fn hint_ids() -> impl Iterator<Item = &'static str> {
    [FILE_NAMES, EXTENSIONS, INTERPRETERS, MODELINES]
        .into_iter()
        .flatten()
        .map(|(_, id)| *id)
        .chain(["xml", "html"])
}

fn lookup(table: &[(&str, &'static str)], key: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, id)| *id)
}

/// A hint from the file name alone. Checked before GtkSourceView's globs, so it overrides them.
pub fn from_file_name(name: &str) -> Option<&'static str> {
    if let Some(id) = lookup(FILE_NAMES, name) {
        return Some(id);
    }
    let (_, extension) = name.rsplit_once('.')?;
    lookup(EXTENSIONS, &extension.to_ascii_lowercase())
}

/// A hint from the first line of the text: a shebang (`#!/usr/bin/env python3`), an XML or
/// HTML declaration, or an Emacs or Vim modeline.
pub fn from_first_line(line: &str) -> Option<&'static str> {
    let line = line.trim_start_matches('\u{feff}').trim_end();
    if let Some(command) = line.strip_prefix("#!") {
        return from_interpreter(command);
    }
    let lower = line.trim_start().to_ascii_lowercase();
    if lower.starts_with("<?xml") {
        return Some("xml");
    }
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return Some("html");
    }
    from_emacs_modeline(line).or_else(|| from_vim_modeline(line))
}

fn from_interpreter(command: &str) -> Option<&'static str> {
    let mut words = command.split_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|word| !word.starts_with('-') && !word.contains('='))?;
    }
    let base = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    lookup(INTERPRETERS, base)
}

/// `-*- mode: python -*-` or `-*- python -*-`.
fn from_emacs_modeline(line: &str) -> Option<&'static str> {
    let start = line.find("-*-")? + 3;
    let end = start + line[start..].find("-*-")?;
    let inner = line[start..end].trim();
    let mode = inner
        .split(';')
        .find_map(|part| {
            let (key, value) = part.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case("mode")
                .then(|| value.trim())
        })
        .unwrap_or(if inner.contains(':') { "" } else { inner });
    lookup(MODELINES, &mode.to_ascii_lowercase())
}

/// `vim: set ft=rust:` or `vim: filetype=sh`.
fn from_vim_modeline(line: &str) -> Option<&'static str> {
    let start = line.find("vim:").or_else(|| line.find("vi:"))?;
    line[start..]
        .split(|c: char| c.is_whitespace() || c == ':')
        .find_map(|word| {
            word.strip_prefix("ft=")
                .or_else(|| word.strip_prefix("filetype="))
        })
        .and_then(|mode| lookup(MODELINES, &mode.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_gtksourceview_does_not_know() {
        assert_eq!(from_file_name("PKGBUILD"), Some("sh"));
        assert_eq!(from_file_name("Cargo.lock"), Some("toml"));
        assert_eq!(from_file_name(".zshrc"), Some("sh"));
        assert_eq!(from_file_name("omarchy-menu.jsonc"), Some("json"));
        assert_eq!(from_file_name("stet.install"), Some("sh"));
        assert_eq!(from_file_name("stet.service"), Some("ini"));
        assert_eq!(from_file_name("NOTES.JSONC"), Some("json"));
        assert_eq!(from_file_name("main.rs"), None);
        assert_eq!(from_file_name("README"), None);
        assert_eq!(from_file_name(""), None);
    }

    #[test]
    fn shebangs() {
        for (line, id) in [
            ("#!/bin/sh", "sh"),
            ("#!/usr/bin/env bash", "sh"),
            ("#!/usr/bin/env -S bash -eu", "sh"),
            ("#! /bin/zsh", "sh"),
            ("#!/usr/bin/python3.12", "python3"),
            ("#!/usr/bin/env python", "python3"),
            ("#!/usr/bin/env LC_ALL=C python3", "python3"),
            ("#!/usr/bin/perl -w", "perl"),
            ("#!/usr/bin/env node", "js"),
            ("#!/usr/bin/env -S deno run", "js"),
            ("#!/usr/bin/awk -f", "awk"),
            ("#!/usr/bin/env fish", "fish"),
            ("\u{feff}#!/bin/bash", "sh"),
        ] {
            assert_eq!(from_first_line(line), Some(id), "{line}");
        }
        for line in ["#!", "#!/usr/bin/env", "#!/opt/tool", "# not a shebang", ""] {
            assert_eq!(from_first_line(line), None, "{line}");
        }
    }

    #[test]
    fn declarations_and_modelines() {
        assert_eq!(
            from_first_line("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            Some("xml")
        );
        assert_eq!(from_first_line("<!DOCTYPE html>"), Some("html"));
        assert_eq!(
            from_first_line("# -*- mode: python; coding: utf-8 -*-"),
            Some("python3")
        );
        assert_eq!(from_first_line(";; -*- makefile -*-"), Some("makefile"));
        assert_eq!(from_first_line("// vim: set ft=rust:"), Some("rust"));
        assert_eq!(from_first_line("# vim: filetype=sh"), Some("sh"));
        assert_eq!(from_first_line("# -*- coding: utf-8 -*-"), None);
        assert_eq!(from_first_line("fn main() {}"), None);
    }

    #[test]
    fn hint_ids_cover_every_table() {
        let ids: Vec<_> = hint_ids().collect();
        for id in [
            "sh",
            "ini",
            "toml",
            "json",
            "python3",
            "xml",
            "html",
            "typescript",
        ] {
            assert!(ids.contains(&id), "{id}");
        }
    }
}
