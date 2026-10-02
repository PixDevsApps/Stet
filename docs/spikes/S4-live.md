# S4: Live checks on Hyprland, with live re-measures of S1, S5 and S6

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Recorded: 2026-09-30, in the user's live Hyprland session, at the user's request ("run the live checks"). Everything else in `docs/spikes/` ran headless under GTK Broadway; this report is the live counterpart.

## Verdicts

| Check | Result | Verdict |
| --- | --- | --- |
| **S6 start** (release build, empty window) | `main` to first frame: median **224.5 ms** (209.5–308.5), 5 runs; spawn to first-frame log line: median 273.6 ms (261.8–358.8) | **PASS** (≤ 400 ms) |
| S6 start with a 10 MB Rust file | median 346.3 ms (326.2–351.0); spawn to log line 395.0 ms (375.8–402.4) | Within 400 ms, barely. The M0 window loads with one `set_text` on the GTK thread; M3's chunked loading replaces it |
| S6 start, binary evicted from the page cache | median 220.3 ms (205.4–237.1); spawn to log line 272.7 ms | Same as warm: the shared GTK libraries stay cached. A true cold start needs root to drop all caches and was not run |
| **Second launch focuses the running window** | Focus moved from another window to Stet for a direct second launch from a shell **and** for a compositor launch (`hl.dsp.exec_cmd`, the path of SUPER+SHIFT+N). Both files opened as tabs in the one window (3 tabs, [screenshot](../../tools/progress/public/m0-window-2026-09-30.png)); closing the window exited the app with 0 | **PASS** |
| **File dialog floats** | `xdg-desktop-portal-gtk`, floating, 875×600, in the automated run and the interactive run; a file was picked successfully | **PASS** |
| Shortcut conflicts with Hyprland binds | None of 82 planned shortcuts is bound by Hyprland (226 binds checked). The only binds without SUPER are XF86 media/brightness keys, PRINT, Alt+Tab (with Shift/Ctrl variants), Ctrl+Alt+Delete ("close all windows") and the lid switch | **PASS** |
| fcitx5 grabs Ctrl+Shift+U | Never reached the app; fcitx5 showed its Unicode entry preedit `U+` (9 preedit updates over two sessions) | **Confirmed**: UPPERCASE needs another key ([ADR-009](../DECISIONS.md#adr-009--classic-editor-keymap-with-omarchy-remaps-2026-09-30)) |
| fcitx5 grabs Ctrl+Alt+Shift+U | Never reached the app | **Confirmed** |
| Ctrl+Space | Reached the app. fcitx5 has one input method here (`keyboard-us`) and no hotkey overrides, so its trigger key passes through | Usable here, not guaranteed with more input methods; completion stays on Ctrl+Enter |
| Other shortcuts | Ctrl+D, Ctrl+Q, Ctrl+Enter, Ctrl+Shift+Up, Alt+Shift+Down and Ctrl+Tab reached the app | **PASS** |
| F-keys (F1, F11, Alt+F3, Alt+Shift+F3) | No event in either session. The follow-up logged every key by name and recorded no F-key and no XF86 key | **INCONCLUSIVE**: either not pressed, or the F-row sends media keys that Hyprland consumes (it binds the XF86 keys). Open question for the user |
| Typing through fcitx5 (`keyboard-us`) | 54 inserts, 52 of them right after a key event | **PASS** (Latin) |
| Compose and dead keys | Not exercised (no non-ASCII commit) | **NOT TESTED** |
| CJK input | No CJK engine is configured in fcitx5 | **NOT POSSIBLE** without installing one |
| **Real-keyboard latency** (harness, small buffer, 144 Hz) | key press to painted frame: median **2.74 ms**, p95 **14.4 ms**, max 18.49 ms (90 keys); buffer change to painted frame: median 2.67 ms, p95 13.95 ms | **PASS** (p95 < 16 ms) |
| SUPER+C | The clipboard received exactly the selected text | **PASS** (copy) |
| SUPER+V | Not verified: the harness could not tell a paste from typing in the first session, and the clipboard held non-text content in the follow-up | **NOT VERIFIED** |
| SUPER+C/V driven by `wtype` | Copied nothing. Omarchy's `clipboard.lua` documents why: a virtual keyboard's held SUPER merges into the injected Ctrl+C at the seat | Not automatable; use a physical keyboard |
| Drag-and-drop from Nautilus | No drag reached the drop box | **NOT TESTED** |
| fcitx5 in a column caret | The caret was switched on, but nothing was typed in its range | **NOT TESTED** |
| Fractional scaling | The monitor runs at scale 1; not switched | **NOT TESTED** (optional) |

### Live re-measure of S1 (typing, 10 MB Rust file, highlighting on)

`STET_SPIKE_ALLOW_LIVE=1 target/release/s1_perf typing-rust`: 3 runs, 50 keystrokes per position per run, release build. Compare with [S1](S1-performance.md) under Broadway.

| Position | Insert to painted frame, median / p95 / max | Broadway (S1) | Verdict |
| --- | --- | --- | --- |
| End of file, highlighting settled | 3.49 / 11.84 / 16.54 ms | 7.7 ms median | **PASS** |
| Middle | 3.25 / 11.96 / 16.48 ms | 8.9 ms | **PASS** |
| Line 20 | 2.70 / 12.10 / 16.30 ms | 5.2 ms | **PASS** |
| Offset 0 (before `#![allow]`) | 5.43 / 9.84 / 16.70 ms | 29.5 ms (FAIL) | **PASS**: the Broadway failure does not reproduce live |
| Line 20, **right after load** | 70.34 / 1338 / 1574 ms; longest gap between frames 1.46–1.58 s; longest main-loop block 126–134 ms | 80–104 ms median, p95 1.45–1.79 s | **FAIL**: reproduces live |
| Offset 0, **right after load** | 76.77 / 1412 / 1730 ms; longest gap 1.54–1.73 s; longest block 133–143 ms | — | **FAIL** |

Highlighting settled about 4.0 s after load (3.94–4.07 s; about 7 s under Broadway). First paint of the 10 MB file: 137–139 ms. The post-open stall is real on Hyprland, which validates [ADR-014](../DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30).

### Live re-measure of S5 (column mode, 10k lines)

`STET_SPIKE_ALLOW_LIVE=1 target/release/s5_column <out.md>`: 1 run, each timing the median of 5 repetitions. The generated table keeps its Broadway wording ("under Broadway", "Broadway starts the frame"); in this run those rows are Wayland. Compare with [S5](S5-column.md).

| Measure | Live result | Verdict |
| --- | --- | --- |
| Insert at column 0 / column 10 across 10k lines, one user action | 28.30 / 33.02 ms | **PASS** (< 200 ms) |
| Undo / redo of those per-line edits, view attached | 238.4 / 237.0 ms (column 10: 248.0 / 241.4 ms) | Over the 200 ms bar, as under Broadway; the bulk-edit rule of the [ADR-003 amendment](../DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30) is needed |
| Same edit as one delete + one insert of all lines | edit 6.03 ms; undo 4.67 ms; redo 4.12 ms | **PASS** |
| One keystroke replicated to 10k lines | 33.32 ms, plus 1.61 ms of frame work; about 3.3 µs per line | Fine for typical rectangles (100 lines ≈ 0.3 ms) |
| Grouped typing burst | 1 undo step, redo 1 | **PASS** |
| Tab stops as GtkSourceView sets them / with Pango units | 5 of 18 positions off / 0 of 18 | Same as under Broadway; [ADR-015](../DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30) confirmed |
| GLib/GTK warnings | 0 | **PASS** |

## Environment

- Hyprland 0.56.2 (Lua configuration), Omarchy 4.0.4; GTK 4.22.4, GtkSourceView 5.20.0, libadwaita 1.9.3; AMD Ryzen 9 5900X; release builds.
- One monitor: DP-2, 3440×1440 at 144 Hz (a frame every 6.94 ms), scale 1.
- Keyboard layout `no` (Norwegian) from the user's `~/.config/hypr/input.lua`, with Omarchy's default `compose:caps`.
- fcitx5 running, one input method (`keyboard-us`), no hotkey overrides in `~/.config/fcitx5/config`; `fcitx5-remote` reported 1 (inactive).
- Other windows open on the workspace; the machine was otherwise idle (load average 0.4–0.5 at the S1 starts).

## Method

**Automated** (windows opened and closed on the user's workspace; the user was asked not to type):

- **S6:** `STET_EXIT_AFTER_MS=700 target/release/stet [file]`, 5 runs per variant. A driver timed from spawn to reading the `first-frame` log line; the app itself logs `main` to first frame. For the evicted variant the driver called `posix_fadvise(POSIX_FADV_DONTNEED)` on the binary before each run.
- **Second launch:** start `stet app/src/window.rs`; move focus to another window with `hyprctl dispatch 'hl.dsp.focus({ window = "address:…" })'`; run `stet README.md` from a shell; record the active window; move focus away again; run `hyprctl dispatch 'hl.dsp.exec_cmd("…/stet Cargo.toml")'`; record the active window; screenshot the window with `grim -g`; close it with `hl.dsp.window.close`.
- **File dialog and clipboard:** `target/release/s4_live --auto` opens its file dialog, reads `hyprctl -j clients` 1.5 s later, cancels the dialog, then selects its first line for a `wtype` SUPER+C/V attempt. The driver sent keys only after confirming that the harness window had focus.
- **Shortcut conflicts:** `hyprctl -j binds`, intersected with the planned keymap.
- **S1, S5:** the commands above, with `STET_SPIKE_ALLOW_LIVE=1`. `s1_perf` now connects its Broadway web client only under Broadway, so it runs live.

**Interactive:** `target/release/s4_live` (then `--follow-up`) shows an editor, a shortcut checklist that ticks keys reaching the app in the capture phase, a five-line column caret, a drop box, a file-dialog button, a clipboard monitor and per-keystroke latency (key press or buffer change to the next `after-paint`). On Finish it writes JSON to `$STET_S4_OUT`. The user ran one full session and one follow-up.

The driver scripts were throwaway session scripts and are not in the repository. **Correction:** the first driver used the pre-0.56 dispatcher syntax (`hyprctl dispatch focuswindow address:…`), which Hyprland 0.56 rejects. Its focus and close steps silently did nothing, so its second-launch result was discarded and the test re-run with the Lua forms above.

## Findings

1. **Hyprland 0.56 takes Lua in `hyprctl dispatch`:** `hl.dsp.focus({ window = "address:…" })`, `hl.dsp.exec_cmd("…")` and `hl.dsp.window.close({ window = "address:…" })`. The old forms are rejected, and ignoring the reply turns the failure silent. Any documented snippet or test script must use the Lua forms, as `omarchy-launch-or-focus` does.
2. **Startup is 4× slower live than under Broadway** (224.5 ms against 56.4 ms), most likely GPU renderer start-up. It is still within the bar, but M1's shell adds work, so keep S6 in every milestone's checks.
3. **The post-open highlighting stall is real on Hyprland** (median 70–77 ms, frames stalled up to 1.73 s for about 4 s). ADR-014 is needed. S1's offset-0 failure was Broadway-only.
4. **fcitx5 takes both UPPERCASE candidates.** Ctrl+Shift+U enters fcitx5's Unicode mode (`U+` preedit), and Ctrl+Alt+Shift+U never arrives. The M5 replacement key must avoid both.
5. **F-keys are unresolved.** The classic editor keymap relies on F3, F2, F7, F8, F11 and F1. If the user's F-row sends media keys by default, Hyprland consumes them before any app sees them, so the default keymap needs an F-key-free path (the palette, alternative accelerators) or the user keeps an Fn-lock.
6. **The user's layout is Norwegian.** `=`, `/`, `\`, `[`, `]`, `{` and `}` are shifted or AltGr keys, so accelerators such as Ctrl+= (zoom) and Ctrl+/ need testing on this layout in M1's keymap audit.
7. **Omarchy's scaling steps are 1, 1.25, 1.6 and 2** (`omarchy hyprland monitor scaling`), not 1.5. Acceptance should use 1.25 and 1.6.
8. The harness itself logged one `Trying to snapshot GtkGizmo … without a current allocation` warning per session (from its own layout, not from Stet).

## Recommendations

- **Go for the GTK stack** ([ADR-002](../DECISIONS.md#adr-002--gtk4--libadwaita--gtksourceview-5-with-a-gpui-fallback-2026-09-30)): nothing measured live argues for GPUI.
- Carry the untested items into the milestones that build them. Drag-and-drop and SUPER+V go into M1's acceptance, with the real app. fcitx5 in a column caret goes into M6. Compose, dead keys and fractional scaling at 1.25 and 1.6 go into the manual acceptance list.
- Ask the user about the F-row (finding 5) before the M1 keymap work.
- Use the Lua dispatcher forms in [INTEGRATIONS.md](../INTEGRATIONS.md) and in any future test script.
