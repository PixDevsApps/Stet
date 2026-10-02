# Upstream note: after Undo and Redo, GTK 4.22 joins the next edit to the redone step

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-01

**Status: present in GTK 4.22.4 and on `main`; not filed.** Stet works around it (below). Found during M6 (column mode).

## What happens

In any `GtkTextBuffer` with undo, a step made of several edits (a user action with two or more changes: a column edit, a Replace All, a paste over a selection) loses its end marker when it is undone and redone. The next edit then joins that step, and one Undo takes back both: the new edit and the redone step.

## Reproduced on 2026-10-01

GTK 4.22.4 (Arch), a plain `Gtk.TextBuffer` without a view, PyGObject, headless (Broadway). Three user actions, each inserting at two places: `ab\nab\n`, then `x` at the start of both lines, then `y` the same way. Then some undos and redos, then a fourth user action inserting `z` on both lines, then one Undo:

| Before the fourth step | One Undo gives | Expected |
| --- | --- | --- |
| No undo or redo | `yxab\nyxab\n` | the same |
| Undo, Redo | `xab\nxab\n` (the `y` step went too) | `yxab\nyxab\n` |
| Undo twice, Redo once | `ab\nab\n` (the `x` step went too) | `xab\nxab\n` |
| Undo twice, Redo twice | `xab\nxab\n` (the `y` step went too) | `yxab\nyxab\n` |

```python
buffer = Gtk.TextBuffer(enable_undo=True)
def step(*inserts):
    buffer.begin_user_action()
    for offset, text in inserts:
        buffer.insert(buffer.get_iter_at_offset(offset), text)
    buffer.end_user_action()
step((0, "ab\nab\n")); step((3, "x"), (0, "x")); step((4, "y"), (0, "y"))
buffer.undo(); buffer.redo()
step((5, "z"), (0, "z")); buffer.undo()   # gives "xab\nxab\n", not "yxab\nyxab\n"
```

## Cause

In `gtk/gtktexthistory.c` (the same on `gtk-4-22` and `main`, read on 2026-10-01):

- `gtk_text_history_end_user_action` ends a group of two or more actions with a **barrier** action, so that later actions don't join the group.
- `gtk_text_history_undo` moves the trailing barrier to the head of the redo queue and then the group in front of it: the redo queue reads group, barrier.
- `gtk_text_history_redo` moves a barrier only when it is at the head of the redo queue, which it never is after an undo; it moves the group back and leaves the barrier behind. The barrier goes back only with the next redo, so after the last redo, or before any further one, the undo queue ends with a group and no barrier.
- `gtk_text_history_begin_user_action` reuses a group at the tail of the undo queue (meant for nested user actions), and `gtk_text_history_push` appends any new action to such a group outside a user action too. So the next edit joins the redone group.

A fix would be for `gtk_text_history_redo` to move the barrier that follows the redone action back with it.

## What Stet does

`app/src/column/edit.rs` (`connect_buffer`), for every editor buffer:

- After a redo that leaves nothing to redo, an empty user action puts the barrier back at once. GTK clears the redo queue at the end of every user action; here it holds nothing but the stray barrier.
- After a redo that leaves more to redo, nothing can be done without clearing the rest of the redo history. The next user action does it as it begins: the `begin-user-action` handler (emitted after the history has begun the user action, which has just joined the redone group) ends the user action and begins it again, as GtkTextView itself does between a deleted selection and the typed text. That closes the redone group with a barrier and starts a new one. The redo history it clears would have been cleared by that user action anyway. An edit outside any user action gets an empty user action before it instead.
- `tests/selftest/column.stet-test` checks a column typing burst after Undo and Redo, after a redone Replace All, after two undos and one redo, and plain typing after two undos and one redo: each is one undo step. Without the workaround the first two fail (M6), and without its second half the last two fail ("undo steps: got 5, wanted 1"; "6 undo steps did not reach typed-after-redo").

## Optional: report it (the user's action)

Open an issue on [GNOME/gtk](https://gitlab.gnome.org/GNOME/gtk/-/issues) with a GNOME GitLab account:

> **Title:** GtkTextHistory: after undo and redo of a grouped action, the next action joins the redone group
>
> `gtk_text_history_undo` moves a group's trailing barrier to the redo queue behind the group, and `gtk_text_history_redo` moves the group back but leaves the barrier at the head of the redo queue. The redone group then has no barrier, so the next user action (`gtk_text_history_begin_user_action` reuses a group at the tail) or edit joins it, and one undo reverts both. Seen in 4.22.4; the code is the same on main. Reproducer: three user actions with two inserts each, undo, redo, a fourth user action, undo: the third action is undone together with the fourth.
