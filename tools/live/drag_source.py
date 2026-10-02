#!/usr/bin/env python3
"""A GTK4 window that drags the given files as a GdkFileList, like Nautilus does.

Used by Stet's live drag-and-drop check together with tools/live/pointer:
    python3 tools/live/drag_source.py /path/to/file [more files]
"""
import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GObject, Gtk  # noqa: E402

PATHS = sys.argv[1:]


def prepare(_source, _x, _y):
    files = Gdk.FileList.new_from_array([Gio.File.new_for_path(p) for p in PATHS])
    return Gdk.ContentProvider.new_for_value(GObject.Value(Gdk.FileList, files))


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="Stet drag source")
    label = Gtk.Label(label="Drag me: " + ", ".join(PATHS), hexpand=True, vexpand=True)
    source = Gtk.DragSource(actions=Gdk.DragAction.COPY)
    source.connect("prepare", prepare)
    label.add_controller(source)
    window.set_child(label)
    window.present()


if __name__ == "__main__":
    application = Gtk.Application(
        application_id="io.github.pixdevsapps.Stet.DragSource",
        flags=Gio.ApplicationFlags.NON_UNIQUE,
    )
    application.connect("activate", activate)
    application.run([])
