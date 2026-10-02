# Upstream note: GTK 4.22 crashes on AT-SPI `AddSelection` in a text view

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-01

**Status: fixed upstream on `main`, not backported to 4.22.** Nothing needs filing; a backport request is optional (below). Found during the M1 live checks. Stet's editor works around it since M6 (below).

## What happens

Any GTK 4.22 app with a `GtkTextView` (Stet's editor is a GtkSourceView, a GtkTextView subclass) segfaults when an assistive technology calls the AT-SPI `Text.AddSelection` method on the text view while it already has a selection.

## Reproduced on 2026-10-01

GTK 4.22.4 (Arch), live Hyprland session.

1. Run a GTK 4 window with a `Gtk.TextView` holding a selection (PyGObject, 10 lines).
2. Call `Atspi.Text.add_selection(text_view_accessible, 0, 5)` from another process.
3. The window process dies with SIGSEGV (exit −11); the AT-SPI caller gets "Remote peer disconnected".

The same happened to Stet's release build. systemd-coredump caught both crashes; the faulting frames are in `libgtk-4.so.1`, called from GIO's D-Bus method dispatch.

`Atspi.Text.set_selection` (the method GTK added in #7739) works and is safe.

## Cause

- `gtk/a11y/gtkatspitext.c`, the `AddSelection` branch (around line 350 in 4.22.4), calls `gtk_accessible_text_get_selection (accessible_text, &n_ranges, NULL)`. The `ranges` argument is documented as `(optional)`.
- `gtk_text_view_accessible_text_get_selection` in `gtk/gtktextview.c` writes `*ranges = g_new (GtkAccessibleTextRange, 1)` without checking for NULL whenever a selection exists, which is a NULL dereference.
- `gtk/gtklabel.c` has the same unconditional write in 4.22.4.

## Upstream state (checked 2026-10-01)

- **Fixed on `main`** by commit `697abcf3` ("a11y: Handle optional args in AccessibleText impls", 2026-08-31), which wraps the write in `if (ranges != NULL)`.
- **Not in `gtk-4-22`:** neither the branch head nor the 4.22.5 tag has the NULL check, so Arch's gtk4 4.22.x keeps the crash until the next stable series.

## What Stet does

- The live acceptance script never calls `add_selection`; it selects with `set_selection`.
- **Mitigation planned in M6:** Stet's own `View` subclass for column mode can re-implement `GtkAccessibleText::get_selection` NULL-safely and chain to GTK for everything else. That stops the crash in Stet even on GTK 4.22.
- **Built in M6 (2026-10-01):** `StetView` adds GtkAccessibleText to its own type again (`g_type_add_interface_static` in `type_init`); GObject starts the vtable as a copy of GtkTextView's and only `get_selection` is replaced, by a function that calls GtkTextView's with storage of its own and hands the ranges out only when asked for (`app/src/column/a11y.rs`). Headless, `column.stet-test` calls the vtable's `get_selection` with a selection and NULL ranges, as `AddSelection` does: TRUE, one range, no crash; with storage it gives the selection's range. Not yet checked: `Atspi.Text.add_selection` from another process against the live build (on the M6 live list in [PLAN.md](../PLAN.md)). Until that passes, the live acceptance script keeps using `set_selection`.

## Optional: ask for a backport (the user's action)

Open an issue or comment on [GNOME/gtk](https://gitlab.gnome.org/GNOME/gtk/-/issues) with a GNOME GitLab account:

> **Title:** Backport 697abcf3 ("a11y: Handle optional args in AccessibleText impls") to gtk-4-22
>
> In GTK 4.22.4, an AT-SPI client calling `Text.AddSelection` on a `GtkTextView` (or `GtkLabel`) that already has a selection crashes the application: `gtkatspitext.c` passes `NULL` for the optional `ranges` argument of `gtk_accessible_text_get_selection`, and `gtk_text_view_accessible_text_get_selection` writes `*ranges` unconditionally. Commit 697abcf3 on main fixes it; it would be good to have it in 4.22.x, since screen readers can trigger it.
