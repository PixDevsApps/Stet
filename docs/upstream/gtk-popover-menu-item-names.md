# Upstream note: GTK 4.22 popover menu items have no accessible name

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-01

**Status: worked around in Stet; not yet reported upstream.** Found during M9's accessibility audit (`tools/live/acceptance.py`, check `a11y_audit`).

## What happens

Every item of a `GtkPopoverMenu` built from a `GMenuModel` (a `GtkMenuButton`'s menu, `AdwTabView`'s tab menu, `GtkTextView`'s context menu with an extra menu) reaches AT-SPI as role `menu item` with an **empty name**, and nothing readable inside it: its children are two unnamed panels, and the label that shows the text is not exposed. A screen reader announces nothing for the item.

## Reproduced on 2026-10-01

GTK 4.22.4, libadwaita 1.9.3 (Arch), live Hyprland session, at-spi2-core from Arch.

1. Open Stet's main menu (a `GtkMenuButton` with a menu model) or its tab menu (`AdwTabView::set_menu_model`).
2. With PyGObject's Atspi, walk the application's tree: the showing `menu item` nodes have `get_name() == ""`; their attributes are `{'toolkit': 'GTK', 'keyshortcuts': …}`.
3. `clear_cache()` and `set_cache_mask(Atspi.Cache.NONE)` on the items change nothing, so the name GTK reports is empty, not a stale cache.

A plain `GtkLabel`, `GtkButton` with a label, or `GtkCheckButton` is named as expected in the same window.

## Cause, as far as we can tell

`GtkModelButton` sets `GTK_ACCESSIBLE_RELATION_LABELLED_BY` to its internal label. The name GTK computes through that relation comes out empty, and because an ARIA "labelled by" outranks a label, an explicit `GTK_ACCESSIBLE_PROPERTY_LABEL` set on the button is ignored while the relation is there. Resetting the relation and setting the label (Stet's workaround) gives the item its text as its name, which supports this reading.

Not checked: whether a minimal application shows the same (a minimal PyGObject probe's popover did not appear on the bus in our attempt), and whether GTK's `main` branch has changed this.

## What Stet does

`app/src/window/a11y.rs`: when the main menu opens, a tab menu is set up, or an editor gets a right click, Stet walks the window's widgets once the menu exists and gives every `GtkModelButton` its own `text` (mnemonic underscores removed) as `GTK_ACCESSIBLE_PROPERTY_LABEL`, after `gtk_accessible_reset_relation (LABELLED_BY)`. The check `a11y_audit` fails if any showing interactive widget is unnamed.

## Optional: report it (the user's action)

Open an issue on [GNOME/gtk](https://gitlab.gnome.org/GNOME/gtk/-/issues):

> **Title:** GtkModelButton (popover menu items from a GMenuModel) has an empty accessible name in 4.22
>
> In GTK 4.22.4 every item of a GtkPopoverMenu created from a GMenuModel is exposed over AT-SPI as `menu item` with an empty name and no named descendants, so Orca reads nothing. The button's LABELLED_BY relation to its internal label seems to resolve to an empty name; setting an accessible label has no effect until that relation is reset. Steps: open any GtkMenuButton menu and read the items' names with Atspi.
