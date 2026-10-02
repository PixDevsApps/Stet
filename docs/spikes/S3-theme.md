# S3 — Theme: Omarchy palette to GtkSourceView scheme and libadwaita CSS, with live reload

> Recorded on 2026-09-30 under the working name Notes. Since [ADR-010](../DECISIONS.md#adr-010-accepted--stet-2026-09-30) the crates `notes-domain`, `notes-infrastructure` and `notes-spikes` are `stet-domain`, `stet-infrastructure` and `stet-spikes`; the `notes:*` style ids are `stet:*`, the generated schemes are `stet-omarchy-<fingerprint>.xml`, and the per-theme override is `stet.toml`. The raw output under `.local/` (`s3-run4.jsonl`, `s3/results-run4.md`) was deleted on 2026-09-30 to free disk space; re-run the spike to regenerate it. Decisions on its recommendations: [PLAN.md, 2026-09-30](../PLAN.md#name-repository-and-spike-changes-approved--2026-09-30).

**Date:** 2026-09-30 · **Milestone:** M0 · **Overall verdict: PASS**, with the caveats and rule change below.

## Purpose

The plan's pass bar for S3 is: "Scheme XML plus CSS variables recolour the whole UI; live switching across 3 themes, including catppuccin-latte (light)". The M0 task sharpened it to two checks: every built-in theme (22) produces a scheme that loads and CSS that parses without errors, and the reload trigger fires reliably.

S3 is also the first real code for [ADR-004](../DECISIONS.md), not a throwaway:

- `domain/src/theme.rs` (pure, no I/O):
  - `Rgb`: parse, `mix` with the same rounding as Omarchy's `mix_color`, and WCAG contrast.
  - `Mode`.
  - `resolve_colors`: a Rust port of the `omarchy-theme-color` alias and fallback cascade.
  - `Palette`: `from_colors` for raw maps, `from_resolved` for script output, built-in `DARK` and `LIGHT` GNOME palettes for running outside Omarchy, and an accent fallback of `accent` → `blue` → built-in accent.
  - `EditorTheme`: role tokens following the [DESIGN.md](../DESIGN.md#theme-tokens) table, plus `scheme_styles`, `style_scheme_xml(id, name)`, `css_colors`, `adwaita_css`, `is_dark`, `fingerprint` and `scheme_id`.
  - Custom scheme styles `notes:smart-highlight` and `notes:search-current`.
  - 28 unit tests, including 2 proptests and 6 insta snapshots (XML and CSS for catppuccin, catppuccin-latte and an ANSI-only legacy palette).
- `infrastructure/src/omarchy/colors.rs`:
  - `parse_colors`: a lenient `colors.toml` parser.
  - `parse_resolved`: parses the script's `key<TAB>value` output.
  - `resolve_palette(file)` and `resolve_palette_with(file, script)`. They run `omarchy-theme-color --file <path> --all` when the script is installed. If the script fails or prints nothing, they fall back to the Rust cascade with a warning, and they honour a `light.mode` file beside `colors.toml`.
  - 10 tests. One of them compares the Rust cascade with the real script and is skipped outside Omarchy.
- `spikes/src/bin/s3_theme.rs`: the measurements below.

**Dependencies added:**
- `insta = { version = "1.48", features = ["json"] }` in the root `[workspace.dependencies]`, and as a dev-dependency of `notes-domain`. It resolved to 1.48.0.
- `tracing` in `notes-infrastructure`, for the fallback warning.

## Method

```
tools/headless.sh 13 cargo run --release -p notes-spikes --bin s3_theme
```

The final run was run 4. It took 7 min 43 s wall-clock and exited 0. Its raw output is in `.local/s3-run4.jsonl`, and its generated tables are in `.local/s3/results-run4.md`; both are ignored by git. The binary does the following.

1. **Themes.** It collects every theme directory with a `colors.toml`: the 22 in `/usr/share/omarchy/themes/` and the 6 in `~/.config/omarchy/themes/`.
2. **Resolver agreement.** For each theme, it resolves the palette 5 times through the script and 5 times through the Rust cascade, then compares the two maps key by key.
3. **Window.** It opens one `adw::Window` on Broadway, containing:
   - an `AdwToolbarView` with an `AdwHeaderBar` (top-bar style RAISED; see recommendation 6);
   - a `sourceview5::View` with the Rust language, line numbers and current-line highlighting, holding a four-line Rust snippet;
   - a status row with a label, a `.card` box and a `.suggested-action` button.
4. **Live switching.** It runs 5 rounds. In each round it switches the same live window through all 28 themes, which is 140 switches in all. Each switch does what the app would do:
   - resolve the palette through the script;
   - build `EditorTheme` and generate the XML and CSS;
   - write `<styles>/notes-omarchy-<fingerprint>.xml` and delete the previous file;
   - `force_rescan` and look up the scheme id;
   - load a new `gtk::CssProvider` with a `parsing-error` handler;
   - `set_style_scheme`, add the new provider at APPLICATION priority and remove the old one;
   - set AdwStyleManager to FORCE_DARK or FORCE_LIGHT;
   - wait for one painted frame.
5. **Verification of each switch:**
   - Every one of the 63 generated styles is looked up through `StyleScheme::style` and its foreground, background, underline colour, bold, italic, strikethrough and underline are compared with what was generated.
   - The buffer's highlighting tags are checked on `fn` (keyword → magenta), `i32` (type → yellow), `42` (number → orange), `// note` (comment → muted and italic) and `"hi"` (string → green).
   - The window is rendered through `GtkWidgetPaintable` and a `gsk::CairoRenderer`, and 8 pixels plus the label's computed colour are compared with the tokens, within ±2 per channel. The checks are: header bar, window background, card, accent button, text area, current line, gutter, current line number, and label colour.
   - GLib warnings and criticals are captured throughout.
6. **Timing.** Each phase above is timed. The frame is timed twice:
   - the GTK work inside the frame, from before-paint to after-paint;
   - the wall clock until the frame. Broadway throttles this to about 1 s while no browser is connected, so it is reported but not used.
7. **Rewritten scheme file.** It rewrites one scheme file under an unchanged id, to see whether `force_rescan` serves the new colours.
8. **Legacy colours.** It loads legacy `@define-color` CSS into a detached provider, to see whether GTK flags it.
9. **Live reload.** It builds a fake `…/state/omarchy/current/` under `target/s3-theme-*/` (deleted afterwards) and watches it with `gio::File::monitor_directory(WATCH_MOVES)`.
   - **Trigger rule:** the plan's rule (`theme.name` CHANGES_DONE_HINT, or RENAMED whose new name is `theme`), plus one extra rule found in run 1: CREATED of `current` itself.
   - **Debounce:** a trailing 150 ms `timeout_add_local_once` that is restarted on every trigger. When it fires, it reads `theme.name`, resolves `theme/colors.toml` and applies the theme to the window.
   - **The swap:** run as a real `bash` process, line for line from `/usr/share/omarchy/bin/omarchy-theme-set`:

     ```bash
     rm -rf next-theme
     mkdir -p next-theme
     cp -r <theme>/* next-theme/
     # a generated file into next-theme
     rm -rf theme
     mv next-theme theme
     echo <name> > theme.name
     ln -nsf <background> background
     ```

     The shell IPC, app restarts and hooks are left out; they never touch `current/`. The real `~/.local/state/omarchy` was not touched: its `theme.name` still reads `dark-music2` with an mtime of 09:47, before this work began.
   - **Scenarios:**
     - 4 swaps, each started at least 700 ms after the previous one settled;
     - `omarchy-theme-refresh` (the same name again, with no background link);
     - 3 swaps back to back in one process;
     - the first-ever write of `theme.name`;
     - 2 swaps with the watch created before `current/` existed.

### Runs

| Run | Change | Outcome |
|---|---|---|
| 1 | first version | Every apply waited about 1 s for a Broadway frame. The header-bar sample failed on the 22 built-in themes and on abstract-neon. The flat `AdwToolbarView` top bar shows `--window-bg-color`, not `--headerbar-bg-color` (recommendation 6); the 5 user themes that passed have a `dark_background` equal to `background`, or within the ±2 tolerance of it. The watch created before `current/` existed missed the first swap. |
| 2 | RAISED top bar, sampled after 1 frame | The samples showed the previous theme's colours, or libadwaita's defaults, so the sampling was premature. This was not a theming failure. |
| 3 | pixels polled until they matched (idle, then up to 4 frames); extra trigger rule | Everything passed with the earlier token mapping. |
| 4 (final) | tokens aligned with the DESIGN.md role table; contrast table added | Everything passed. The tables below come from this run. |

## Environment

**Hardware:** AMD Ryzen 9 5900X 12-Core Processor, `nproc` 24, 62.7 GiB RAM (MemTotal 65755304 kB).

**Software:**
- Linux 7.2.5-3-omarchy, Omarchy 4.0.4-1.
- From `pkg-config`: gtk4 4.22.4, gtksourceview-5 5.20.0, libadwaita-1 1.9.3, glib-2.0 2.88.3.
- rustc 1.98.1; crates gtk4 0.11.5, libadwaita 0.9.2, sourceview5 0.11.2; release profile (thin LTO).

**Display:** GTK Broadway through `tools/headless.sh` (display :13), with a private D-Bus, no browser attached, `GTK_A11Y=none` and `GSETTINGS_BACKEND=memory`. Rendering for the pixel samples used `gsk::CairoRenderer`.

## Results

### Pass bars

| Measure | Result | Pass bar | Verdict |
|---|---|---|---|
| Scheme loads for every built-in theme | 22/22 built-in (and 6/6 user) themes load by id through `StyleSchemeManager` in 5/5 rounds | 22/22 | **PASS** |
| CSS without parse errors | 0 `parsing-error` signals and 0 GLib warnings or criticals in 140 loads | none | **PASS** |
| Styles resolve with the expected colours | all 63 generated styles, including `notes:smart-highlight` and `notes:search-current`, match in 140/140 switches | — | **PASS** |
| Applied to a realized view | the keyword, type, number, comment (italic) and string tags carry the expected colours in 140/140 switches | — | **PASS** |
| Recolours the UI, live, across 3 themes including catppuccin-latte | the same window switched 140 times across 28 themes, including 5 light built-ins (catppuccin-latte, flexoki-light, lupine, rose-pine, white); all 9 samples matched in 140/140 switches | the whole UI | **PASS** for the sampled surfaces; popovers, dialogs, tab bar and find bar were not rendered (see caveats) |
| Reload trigger fires once per swap after a 150 ms debounce | 4/4 swaps, the refresh and the first `theme.name` write each gave exactly 1 reload, 150.1 ms after the last trigger, with the new theme painted | fires reliably | **PASS** |
| 3 swaps back to back in one 30 ms process | 1 reload, with the final theme (nord) | — | **PASS** |
| Watch created before `current/` exists | The plan's rule missed the first swap (run 1). Only `CREATED current` arrived, after 1992 ms in run 1 and 940 ms in run 4, because GLib polls for a missing watched directory. With the extra rule, both swaps reloaded. | not in the bar | **PASS with the extra rule** (recommendation 1) |
| Rust fallback cascade matches `omarchy-theme-color --all` | identical maps for all 28 themes, and for the 3 synthetic cases in `builtin_resolver_matches_omarchy_theme_color` (short names `bg`/`fg`, `purple`, `cursor` override, ANSI-only, `light.mode`) | — | **PASS** |

### Themes

Switch cost is the time to resolve, generate, load and apply, plus the GTK work inside the first frame. It excludes Broadway's roughly 1 s frame wait. Light themes cost about 20 ms more because every switch in the loop crosses dark ↔ light (see timing).

| Theme | Source | Mode | Script = built-in resolver | Keys the file lacks (filled by the cascade) | Scheme loads | Styles match | Syntax tags | CSS errors | Pixels match (rounds) | Pixels correct (worst round) | Switch cost: resolve → apply + first-frame GTK work, ms median (min–max) | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| catppuccin | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 13.9 (13.7–32.9) | PASS |
| catppuccin-latte | built-in | light | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 33.9 (32.9–34.5) | PASS |
| ethereal | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 33.6 (33.4–34.7) | PASS |
| everforest | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.1 (13.8–14.7) | PASS |
| flexoki-light | built-in | light | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 34.4 (33.0–36.0) | PASS |
| gruvbox | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 34.0 (33.1–34.9) | PASS |
| hackerman | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.9 (14.2–15.5) | PASS |
| kanagawa | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.0 (13.7–15.2) | PASS |
| last-horizon | built-in | dark | identical | selection_foreground, orange, brown | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 16.4 (16.1–17.9) | PASS |
| lumon | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.2 (13.9–14.5) | PASS |
| lupine | built-in | light | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 33.7 (33.2–35.9) | PASS |
| matte-black | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 34.1 (33.1–35.2) | PASS |
| miasma | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.2 (13.7–14.4) | PASS |
| nord | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.0 (13.5–14.0) | PASS |
| osaka-jade | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.5 (14.2–14.6) | PASS |
| retro-82 | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.1 (13.8–14.4) | PASS |
| ristretto | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 13.9 (13.9–14.6) | PASS |
| rose-pine | built-in | light | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 34.5 (32.5–35.7) | PASS |
| solitude | built-in | dark | identical | selection_foreground, orange, brown | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 37.1 (35.9–37.5) | PASS |
| tokyo-night | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 13.9 (13.7–14.3) | PASS |
| vantablack | built-in | dark | identical | selection_foreground | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.2 (13.3–14.9) | PASS |
| white | built-in | light | identical | selection_foreground, orange, brown | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 37.0 (35.9–39.1) | PASS |
| abstract-neon | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 34.2 (33.0–35.9) | PASS |
| dark-music | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.1 (13.4–14.5) | PASS |
| dark-music2 | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.6 (14.0–15.1) | PASS |
| dark-music2-custom | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.3 (13.8–14.7) | PASS |
| orange-glow | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.0 (13.4–14.6) | PASS |
| wallhaven-pomlq9 | user | dark | identical | — | 5/5 | yes | yes | 0 | 5/5 | +2 frame(s) | 14.1 (13.7–14.7) | PASS |

### Timing (140 switches, 5 per theme)

| Phase | ms median (min–max) |
|---|---|
| resolve: `omarchy-theme-color --all` (process spawn) | 7.8 (7.0–10.9) |
| generate: palette → tokens → XML + CSS | 0.045 (0.040–0.072) |
| scheme: write XML, delete old, force_rescan, lookup | 3.0 (2.6–4.2) |
| css: CssProvider::load_from_string | 0.023 (0.020–0.056) |
| apply: set_style_scheme, swap provider, StyleManager (same mode) | 0.2 (0.1–19.7) |
| apply, when the StyleManager switches dark ↔ light | 20.2 (19.1–21.9) |
| GTK work in the first frame, before-paint → after-paint (same mode) | 3.1 (2.7–3.8) |
| GTK work in the first frame, when switching dark ↔ light | 3.1 (2.7–3.9) |
| wall clock until the first frame (Broadway-throttled) | 990.1 (935.7–991.4) |
| switch cost: resolve → apply + first-frame GTK work | 14.6 (13.3–39.1) |
| wall clock including the Broadway frame wait | 1001.5 (965.2–1002.4) |
| per-theme median, resolve via script only | 7.1 (6.8–9.8) |
| per-theme median, built-in Rust resolver only | 0.026 (0.023–0.038) |

**What the timing shows:**
- **Theme switch cost:**
  - The script spawn (about 7–8 ms) dominates a same-mode switch.
  - The scheme rescan costs 3 ms, and the GTK frame work 3 ms.
  - Everything else costs less than 0.3 ms.
- **Mode switches:**
  - Switching dark ↔ light makes `AdwStyleManager::set_color_scheme` cost about 20 ms, synchronously, as libadwaita swaps its stylesheet.
  - The frame work itself did not grow.
  - The same-mode apply maximum of 19.7 ms is most likely the very first apply, which also changed the mode from libadwaita's default. catppuccin's switch-cost maximum of 32.9 ms points the same way.

### When the rendered window showed the new theme

| Pixels correct | Theme switches |
|---|---|
| +2 frame(s) | 140 |

The label's computed colour (`gtk_widget_get_color`, no rendering involved) was already the new `--window-fg-color` right after the first frame in 140 of 140 theme switches.

In every one of the 140 switches, the image rendered through `GtkWidgetPaintable` matched 2 frames after the first frame, never earlier or later, while the computed style was already new. The most likely cause is that `GtkWidgetPaintable` updates its image late, not that GTK repaints late. Broadway without a client cannot show the real on-screen frame, so this must be checked by eye on Hyprland.

### Contrast of the generated roles

This is informational: the plan sets no contrast bar. DESIGN.md asked S3 to check contrast. Ratios are WCAG 2; **bold** marks values below 4.5 for text or below 3.0 for comments and syntax colours.

| Theme | Text | Comment | Weakest syntax colour | Selection text | Text on accent | Current match | Popover text | Current line vs background |
|---|---|---|---|---|---|---|---|---|
| catppuccin | 11.3 | **2.5** | red 7.1 | 6.3 | 7.8 | 12.9 | 13.1 | 1.30 |
| catppuccin-latte | 7.1 | **1.9** | yellow **2.3** | 5.2 | **4.3** | **3.1** | 5.6 | 1.17 |
| ethereal | 13.7 | 4.9 | blue 5.6 | 9.2 | 5.6 | 10.9 | 14.2 | 1.15 |
| everforest | 7.4 | **1.6** | red 4.5 | 5.6 | 5.7 | 6.8 | 10.1 | 1.15 |
| flexoki-light | 18.6 | **2.0** | yellow **2.3** | 12.0 | 6.4 | 8.1 | 14.8 | 1.24 |
| gruvbox | 8.2 | **2.3** | red 4.7 | 4.9 | 5.9 | 6.7 | 10.0 | 1.27 |
| hackerman | 17.5 | **1.6** | blue 7.2 | 13.6 | 15.0 | 14.5 | 18.1 | 1.11 |
| kanagawa | 11.3 | **2.2** | red 3.2 | 8.2 | 11.3 | 6.8 | 13.0 | 1.26 |
| last-horizon | 19.1 | **2.5** | yellow 3.3 | 5.9 | 7.3 | 5.9 | 19.7 | 1.18 |
| lumon | 12.1 | **1.7** | red 4.0 | 10.7 | 8.8 | 5.9 | 14.4 | 1.13 |
| lupine | 15.4 | **2.6** | yellow 4.6 | 13.6 | 4.8 | 4.6 | 12.0 | 1.04 |
| matte-black | 10.1 | **1.5** | yellow **2.9** | 7.7 | 7.4 | **3.5** | 10.7 | 1.12 |
| miasma | 8.8 | **2.8** | red **2.3** | 6.5 | **3.9** | **3.9** | 10.4 | 1.14 |
| nord | 9.2 | **1.7** | red 3.1 | 6.4 | 4.6 | 8.0 | 12.6 | 1.24 |
| osaka-jade | 9.7 | **2.9** | orange 4.2 | 8.2 | 4.8 | 4.7 | 10.7 | 1.37 |
| retro-82 | 13.4 | **3.0** | green 4.0 | 7.0 | 9.3 | 6.3 | 14.8 | 1.15 |
| ristretto | 10.9 | **2.8** | red 5.4 | 7.7 | 6.3 | 9.9 | 13.3 | 1.17 |
| rose-pine | 6.7 | **1.5** | yellow **2.1** | 5.3 | **3.1** | **3.2** | 5.3 | 1.10 |
| solitude | 11.6 | **2.2** | red **2.8** | 4.9 | 4.7 | 13.4 | 12.3 | 1.17 |
| tokyo-night | 8.1 | **1.9** | cyan 5.4 | 8.3 | 6.8 | 8.5 | 9.1 | 1.17 |
| vantablack | 21.0 | 4.9 | blue 6.3 | 17.4 | 6.3 | 13.3 | 20.1 | 1.14 |
| white | 21.0 | 3.9 | yellow 8.9 | 11.5 | 5.1 | 8.9 | 17.1 | 1.19 |
| abstract-neon | 15.0 | 3.6 | blue 4.9 | **1.2** | 4.9 | 16.7 | 15.8 | 1.15 |
| dark-music | 17.4 | 3.5 | blue 4.7 | **1.2** | 4.7 | 5.3 | 17.4 | 1.13 |
| dark-music2 | 16.6 | 3.7 | cyan 4.6 | **1.2** | 4.7 | 6.6 | 16.6 | 1.12 |
| dark-music2-custom | 17.4 | 3.5 | blue 4.7 | **1.2** | 4.7 | 5.3 | 17.4 | 1.13 |
| orange-glow | 8.6 | 3.5 | blue 4.9 | **1.2** | 4.9 | 15.6 | 8.6 | 1.08 |
| wallhaven-pomlq9 | 15.3 | 3.1 | blue 4.9 | **1.2** | 4.9 | 17.4 | 15.6 | 1.14 |

**What the contrast table shows:**
- **Comments.** In 19 of 22 built-in themes, comments (`muted`) are below 3:1. This is the theme authors' `muted`, and `helix.toml.tpl` uses it the same way.
- **Weak syntax colours:**
  - Yellow in catppuccin-latte, flexoki-light, rose-pine and matte-black (2.1–2.9). This affects `def:type` and the current search match.
  - Red in miasma (2.3) and solitude (2.8).
- **Text on accent** is below 4.5 in catppuccin-latte (4.3), miasma (3.9) and rose-pine (3.1). For these mid-tone accents, both candidates, `background` and `foreground`, fall short.
- **Current line vs background:** lupine's is 1.04, so its current-line highlight is barely visible.
- **User themes.** All six user themes set `selection_foreground` at 1.2:1 against their own selection. That is the themes' own choice.

### Scheme id, cache and legacy CSS

Raw result (id `notes-s3-stale`, written first from tokyo-night, then rewritten from catppuccin-latte):

- text background on the first load: `#1a1b26`;
- after rewriting the file and `force_rescan`: `#eff1f5`;
- same object: false;
- the first object still reports `#1a1b26`.

- **Rewritten file, same id.** After the file under the same id was rewritten, `force_rescan` returned a **new** `StyleScheme` object with the new colours. The old object kept the old colours. So the manager does not serve a stale scheme, but every buffer must be given the new object with `set_style_scheme`.
- **Legacy `@define-color`.** `@define-color window_bg_color …; window { background-color: @window_bg_color; }` produced **no** parsing messages in GTK 4.22.4, not even a deprecation warning. The generated CSS doesn't need it, because the CSS variables alone recoloured every sampled surface.

### Live reload trigger

| Scenario | Swap process | Events | Triggers | Reloads (expected) | Reloaded theme | Trigger → reload | UI repainted | Verdict |
|---|---|---|---|---|---|---|---|---|
| swap 1 → catppuccin-latte | 20 ms | 10 | 2 | 1 (exactly 1) | catppuccin-latte | 150 ms | yes | PASS |
| swap 2 → gruvbox | 20 ms | 10 | 2 | 1 (exactly 1) | gruvbox | 150 ms | yes | PASS |
| swap 3 → flexoki-light | 20 ms | 10 | 2 | 1 (exactly 1) | flexoki-light | 150 ms | yes | PASS |
| swap 4 → catppuccin | 20 ms | 9 | 2 | 1 (exactly 1) | catppuccin | 150 ms | yes | PASS |
| refresh (same theme, no background) | 11 ms | 7 | 2 | 1 (exactly 1) | catppuccin | 150 ms | yes | PASS |
| burst of 3 swaps in one process | 30 ms | 29 | 6 | 1 (at least 1, last = nord) | nord | 150 ms | yes | PASS |
| first write of theme.name | 322 ms | 11 | 2 | 1 (exactly 1) | kanagawa | 150 ms | yes | PASS |

Watch created before `current/` exists (not part of the pass bar; the swaps run 10 s apart):

| Scenario | Swap process | Events | Triggers | Reloads (expected) | Reloaded theme | Trigger → reload | UI repainted | Verdict |
|---|---|---|---|---|---|---|---|---|
| watch created before current/ exists, swap 1 | 10 ms | 2 | 1 | 1 (exactly 1 (not required)) | everforest | 150 ms | n/a | PASS |
| watch created before current/ exists, swap 2 | 20 ms | 10 | 2 | 1 (exactly 1 (not required)) | osaka-jade | 150 ms | n/a | PASS |

**Observed event sequence for one `omarchy-theme-set` swap.** Times are from the start of the swap process; the triggers are marked.

**swap 1 → catppuccin-latte** (catppuccin-latte):

```
     4.0 ms  CREATED            next-theme
     4.0 ms  CHANGES_DONE_HINT  next-theme
     6.8 ms  DELETED            theme
     8.0 ms  RENAMED            next-theme → theme  ← trigger (plan: renamed to theme)
     8.1 ms  CHANGED            theme.name
     8.2 ms  CHANGED            theme.name
     8.2 ms  CHANGES_DONE_HINT  theme.name  ← trigger (plan: theme.name)
    10.7 ms  CREATED            Cu4s1NKq
    10.8 ms  CHANGES_DONE_HINT  Cu4s1NKq
    10.8 ms  RENAMED            Cu4s1NKq → background
   158.2 ms  RELOAD             theme.name=catppuccin-latte (150.1 ms after the last trigger)
```

**first write of theme.name** (kanagawa):

```
     3.1 ms  DELETED            theme.name
   306.1 ms  CREATED            next-theme
   306.1 ms  CHANGES_DONE_HINT  next-theme
   308.7 ms  DELETED            theme
   309.7 ms  RENAMED            next-theme → theme  ← trigger (plan: renamed to theme)
   309.9 ms  CREATED            theme.name
   309.9 ms  CHANGED            theme.name
   309.9 ms  CHANGES_DONE_HINT  theme.name  ← trigger (plan: theme.name)
   312.5 ms  CREATED            CuZXrLwi
   312.5 ms  CHANGES_DONE_HINT  CuZXrLwi
   312.6 ms  RENAMED            CuZXrLwi → background
   460.0 ms  RELOAD             theme.name=kanagawa (150.1 ms after the last trigger)
```

**What the event sequence shows:**
- **Triggers per swap.** Each swap produces two triggers about 0.2 ms apart: RENAMED `next-theme → theme`, then `theme.name` CHANGES_DONE_HINT. The 150 ms debounce collapses them into one reload.
- **Events the rule must ignore:**
  - `DELETED theme` comes *before* the rename. Reading `theme/` at that moment would find nothing.
  - `ln -nsf` in coreutils creates a temporary name (`Cu…`) and RENAMEs it to `background`.
  - `next-theme` is CREATED.
- **`CHANGED theme.name`** arrives once or twice per swap (swap 4 and the second burst swap had one), so it must not be counted.
- **First write of `theme.name`.** The first-ever write gives CREATED, CHANGED, then CHANGES_DONE_HINT, and the rule still fires.
- **The burst.** The three swaps' triggers fell within about 15 ms, giving one reload with the final theme.
- **`colors.toml` at reload time.** No reload found `theme/colors.toml` missing.

**Watch created before `current/` existed.** In the first swap, only GLib's late `CREATED current` / `CHANGES_DONE_HINT current` arrived, at 940 ms; the swap's own events were lost. The extra rule caught it:

**watch created before current/ exists, swap 1** (everforest):

```
   939.6 ms  CREATED            current  ← trigger (extra: current/ appeared)
   939.6 ms  CHANGES_DONE_HINT  current
  1089.7 ms  RELOAD             theme.name=everforest (150.1 ms after the last trigger)
```

## Caveats

- **Broadway, not Wayland.**
  - Rendering was Broadway plus a Cairo re-render of the widget tree for sampling, not the GL or Vulkan renderer on Hyprland.
  - Broadway throttles frames to about 1 per second without a connected browser. That is why the wall-clock frame numbers are not meaningful and only the GTK work inside the frame is reported.
  - The 2-frame `GtkWidgetPaintable` lag is not proven to be what reaches the screen.
  - Keystroke-to-paint and theme-switch smoothness on real Hyprland must be re-measured manually by the user (TESTING.md manual acceptance item 1: live switching across 5 themes including a light one, plus `omarchy-theme-refresh`).
- **The whole UI was not rendered.** The samples covered the header bar (RAISED), window background, card, suggested button, label colour, text area, current line, gutter and current line number. Popovers, dialogs, the tab bar, the find bar, banners, toasts, scrollbars, the completion popup and focus rings were not rendered. The CSS for popovers, dialogs, error, warning and success is generated and parses, but has not been seen on screen.
- **Square chrome.** `--window-radius: 0px` parsed, but under Broadway there are no tiled or rounded CSD corners to check.
- **The swap was reproduced, not run.** It was a line-for-line reproduction of `omarchy-theme-set`, not the script itself. `omarchy-theme-set-templates` was represented by one generated file in `next-theme`; it writes only inside `next-theme`, which the non-recursive watch does not see.
- **Fake `current/` on a different subvolume.** It was on the project's btrfs subvolume (`target/`), not on `~/.local/state`. inotify semantics are the same.
- **Timing only in-process.** Timing is per switch inside one process; cold-start cost is S6. The script spawn time depends on bash and awk startup. The themes without `orange` (last-horizon, solitude, white) spawn extra awk processes; last-horizon, a same-mode switch, costs 16 ms against about 14 ms.
- **The contrast table uses DESIGN.md's proposed role mapping.** It changes if M1 changes that mapping.

## Recommendations for the plan

1. **Trigger rule (ADR-004).** Keep the plan's rule; it was exact in every scenario. Add one more trigger: **CREATED on the watched `current/` itself.** GLib polls for a watched directory that does not exist yet (the event arrived after 0.9–2 s), so the first swap after `current/` appears is otherwise lost. The plan's "poll every 2 s if the watch can't be set up" is not needed for that case: `monitor_directory` succeeded on the missing path. Also:
   - After (re)starting the watch, compare `theme.name` and the `colors.toml` fingerprint with what was last applied.
   - Never react to `CHANGED`, `DELETED theme`, `next-theme` or `background`.
2. **Keep the 150 ms debounce.** Both triggers of a swap arrive about 0.2 ms apart, and a 3-swap burst collapsed to one reload with the right theme.
3. **Keep calling `omarchy-theme-color`, but off the GTK thread.** Its 7–11 ms spawn is the largest cost of a switch, and "nothing blocks the GTK thread". The Rust cascade gave identical results in 0.03 ms. Keep the conditional equivalence test in CI to catch drift when Omarchy changes the script.
4. **Set the StyleManager colour scheme only when the mode changes.** A dark ↔ light change costs about 20 ms, synchronously; same-mode switches cost 0.2 ms.
5. **Scheme ids.**
   - A unique id is not needed to defeat caching, because `force_rescan` re-reads a rewritten file. The fingerprint id is still useful for the `$XDG_CACHE_HOME/<name>/styles/` file names.
   - Keep exactly one generated file in the search path: delete the old one before `force_rescan`, which the spike did at 3 ms per switch.
   - Always call `set_style_scheme` with the new object on every buffer, including split views.
6. **Header bar colour (DESIGN.md "Tab strip, status bar → `--headerbar-bg-color`").** `AdwToolbarView` defaults to FLAT bars, which show `--window-bg-color`. Use `ToolbarStyle::Raised` (or RAISED_BORDER) for the tab strip and status bar, or style them explicitly. Settle this in the M1 mock.
7. **Contrast guards.** Consider two small `EditorTheme` changes, both pure and testable:
   - Clamp the current line when its contrast with `background` is below about 1.1, mixing toward `foreground` (lupine 1.04).
   - Add black and white as fallback candidates for text on accent, the current match, and error, warning and success when neither `background` nor `foreground` reaches 4.5 (rose-pine 3.1, miasma 3.9).

   Keep syntax colours faithful to the theme; comments below 3:1 are the theme's own `muted`. A per-theme `notes.toml` override (already in ADR-004) is the escape hatch.
8. **Snapshots.** The plan asks for insta snapshots of all 22 built-in `colors.toml` files. S3 snapshots 3 inline fixtures. In M1, copy the 22 files into `tests/fixtures/themes/` (the `omarchy` package is licensed MIT according to `pacman -Qi`) and snapshot them all.
9. **Re-check on Hyprland.** This is the one thing Broadway could not show. Do a visual live switch, including the paint lag and a popover, dialog and find bar in a light theme, in the S4 or M1 live session.
