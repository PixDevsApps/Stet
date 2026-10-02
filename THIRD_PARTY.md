# Third-party notices

Stet's own code, its icons (`assets/brand/`, `assets/icons/`) and its test fixtures are MIT
licensed; see [LICENSE](LICENSE). Asset provenance is in [docs/ASSETS.md](docs/ASSETS.md).

## System libraries

Stet links dynamically to system libraries that Arch Linux packages and licenses separately:
GTK 4, libadwaita, GtkSourceView 5, GLib, Pango, Cairo, graphene, PCRE2 and fontconfig (LGPL,
MIT or BSD-style licenses; GtkSourceView's language definitions keep their own licenses).
Stet does not bundle them or restrict their replacement. See `/usr/share/licenses/` and each
package's metadata for the installed versions. At run time Stet may also call Omarchy's
`omarchy-theme-color` script and fontconfig's `fc-match`; neither is bundled.

## Rust crates

The Rust crates compiled into the `stet` binary, with their declared licenses and
repositories, are listed in `packaging/dependency-licenses.json`. It is generated from the
locked Cargo graph by `tools/license-report.py`, which follows the `stet` package's normal
dependencies (proc-macro crates and their dependencies included, which over-reports slightly)
and copies each crate's license texts into `packaging/licenses/<crate>-<version>/`. The package
installs both under `/usr/share/licenses/stet/`.

At the time of writing (2026-10-01, after M2 and M5: 130 crates) every recorded crate is
available under MIT, Apache-2.0 (some with the LLVM exception), Unlicense, Unicode-3.0,
BSD-3-Clause (`encoding_rs`'s data, with MIT or Apache-2.0 for its code) or Zlib (`foldhash`)
terms, several under a choice of them; the JSON file is the authoritative list.
