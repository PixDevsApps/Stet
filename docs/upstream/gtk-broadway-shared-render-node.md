# Upstream note: GTK 4.24 Broadway aborts on a render node that appears twice in a frame

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-11-07

**Status: not reported upstream; no workaround in Stet. The user chose on 2026-11-07 to wait for a GTK fix.** Found on 2026-11-07 when Stet's headless self-tests stopped passing after the system's update to GTK 4.24.1 ([PLAN.md](../PLAN.md#stet-140-a--button-for-a-new-tab--2026-11-07)).

## What happens

With a Broadway client connected, `gtk4-broadwayd` aborts a few frames after Stet's window appears:

```
Gdk:ERROR:../gtk/gdk/broadway/broadway-output.c:458:append_node_ops: assertion failed: (!reused_node->consumed)
```

Stet's self-tests connect a minimal client (`unpace-frames`, `app/src/selftest/broadway.rs`) so that frames are painted at once. The 16 scripts that do so stop there; the scripts that don't connect a client never reach the server's node diffing, and pass.

## Reproduced on 2026-11-07

gtk4 4.24.1, libadwaita 1.10.0, gtksourceview5 5.22.0 (Arch), headless through `tools/headless.sh`. The same scripts passed on 2026-10-02 with gtk4 4.22.4, libadwaita 1.9.3 and gtksourceview5 5.20.0; the update was installed on 2026-10-18. The crash happens on Stet 1.3.4's code too.

A temporary probe in the self-test (not committed) snapshotted the window through `GtkWidgetPaintable` and walked the render tree with `gsk_render_node_get_children`, looking for one node pointer in two places. Stet's window had 109 nodes, and one `GskFillNode` appeared in four sibling branches. Probing only the status bar gave the same three repeats: they are the four `GtkMenuButton` arrows (language, line ending, encoding, indentation), the same symbolic icon at the same size and colour. GTK 4.24 draws symbolic icons as filled paths and gives each drawing of the same icon the same cached node.

## Cause, as far as we can tell

`gsk/broadway/gskbroadwayrenderer.c` identifies nodes by pointer. A node that was in the previous frame is sent as "reuse id N"; a node that appears twice in the new frame is sent as "reuse id N" twice. `broadway-output.c` expects each reused node to appear once in the tree (the comment at the assertion says so) and aborts on the second. Between 4.22.4 and 4.24.1 these two files and `broadway-server.c` changed only trivially, so the new part is that GTK now shares a node within one frame. Any GTK 4.24 application that shows one symbolic icon twice should crash a connected `gtk4-broadwayd` the same way; not checked with a minimal application or a browser.

`GSK_RENDERER=cairo` doesn't avoid it: the server then aborts on the assertion before it, `(node == reused_node)` at line 456.

## Ways around it for Stet's tests

- **Run the self-tests without a client.** Frames are then painted once a second: `focus.stet-test` passed in 106 s and `session`, `toolbox` passed, but `column.stet-test` failed two hit-test steps.
- **Run them in a headless Wayland compositor** (for example `weston --backend=headless`) instead of Broadway. This needs the compositor installed, and `tools/headless.sh` rewritten with the same isolation.
- **Wait for a GTK fix.** Not checked: whether GTK's `main` branch changed this.

## Optional: report it (the user's action)

Open an issue on [GNOME/gtk](https://gitlab.gnome.org/GNOME/gtk/-/issues):

> **Title:** Broadway: gtk4-broadwayd aborts with `!reused_node->consumed` when a render node appears twice in a frame (4.24)
>
> In GTK 4.24.1, the same symbolic icon shown in several widgets (for example the arrows of several GtkMenuButtons) gives one cached GskFillNode several parents in one frame. GskBroadwayRenderer sends each occurrence as a reuse of the same node id, and broadway-output.c's `append_node_ops` aborts on the second (`assertion failed: (!reused_node->consumed)`). Worked in 4.22.4. Steps: show four GtkMenuButtons side by side under the Broadway backend with a browser connected, and change anything in the window.
