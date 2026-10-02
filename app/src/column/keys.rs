//! Keyboard and pointer input for column mode.
//!
//! - On the view, in the capture phase and ahead of GtkSourceView's own key handler (which
//!   feeds the input method first): Escape and plain arrows leave column mode at the cursor
//!   corner, Tab types a tab on every line, Enter leaves column mode and inserts its line break
//!   as usual. Backspace and Delete arrive through GtkTextView's bindings (`backspace`,
//!   `delete-from-cursor`), which the view overrides.
//! - On the view, a capture-phase drag gesture: Alt+drag (and Alt+click) selects a rectangle;
//!   a plain press ends column mode and goes on to GtkTextView.
//! - On the window, a capture-phase controller that runs before the application accelerators
//!   and closes an open typing burst on every Ctrl, Alt or Super chord, navigation key, Escape
//!   and button press, so that nothing acts while the burst's user action is open (ADR-016).
//!   Focus leaving the view, the idle timeout, the end of column mode and every registry action
//!   (`super::before_action`) close it too.

use super::view::{Collapse, StetView};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use std::cell::RefCell;

thread_local! {
    /// The view whose typing burst is open; at most one, the focused one.
    static OPEN_BURST: RefCell<Option<glib::WeakRef<StetView>>> = const { RefCell::new(None) };
    /// The windows that have the burst-closing controller.
    static HOOKED: RefCell<Vec<glib::WeakRef<gtk::Window>>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn set_open_burst(view: &StetView) {
    let previous = OPEN_BURST.with(|open| open.replace(Some(view.downgrade())));
    if let Some(previous) = previous.and_then(|weak| weak.upgrade())
        && previous != *view
    {
        previous.close_burst();
    }
}

pub(super) fn clear_open_burst(view: &StetView) {
    OPEN_BURST.with(|open| {
        let mut open = open.borrow_mut();
        if open.as_ref().and_then(glib::WeakRef::upgrade).as_ref() == Some(view) {
            *open = None;
        }
    });
}

/// Closes whatever typing burst is open.
pub fn close_open_burst() {
    let open = OPEN_BURST.with(|open| open.borrow().as_ref().and_then(glib::WeakRef::upgrade));
    if let Some(view) = open {
        view.close_burst();
    }
}

/// The names of the view's column-key controller and the window's burst-closing controller.
pub const VIEW_KEYS: &str = "stet-column-keys";
pub const WINDOW_EVENTS: &str = "stet-column-burst";

const NAVIGATION: &[gdk::Key] = &[
    gdk::Key::Escape,
    gdk::Key::Left,
    gdk::Key::Right,
    gdk::Key::Up,
    gdk::Key::Down,
    gdk::Key::Home,
    gdk::Key::End,
    gdk::Key::Page_Up,
    gdk::Key::Page_Down,
    gdk::Key::KP_Left,
    gdk::Key::KP_Right,
    gdk::Key::KP_Up,
    gdk::Key::KP_Down,
    gdk::Key::KP_Home,
    gdk::Key::KP_End,
    gdk::Key::KP_Page_Up,
    gdk::Key::KP_Page_Down,
];

const CHORD: gdk::ModifierType = gdk::ModifierType::CONTROL_MASK
    .union(gdk::ModifierType::ALT_MASK)
    .union(gdk::ModifierType::SUPER_MASK)
    .union(gdk::ModifierType::META_MASK)
    .union(gdk::ModifierType::HYPER_MASK);

/// Whether a key press closes a typing burst: a Ctrl, Alt or Super chord (not the modifier
/// key itself), a navigation key or Escape. AltGr is not a chord: it types characters.
pub fn closes_burst(key: gdk::Key, state: gdk::ModifierType) -> bool {
    let modifier = matches!(
        key,
        gdk::Key::Control_L
            | gdk::Key::Control_R
            | gdk::Key::Alt_L
            | gdk::Key::Alt_R
            | gdk::Key::Super_L
            | gdk::Key::Super_R
            | gdk::Key::Meta_L
            | gdk::Key::Meta_R
            | gdk::Key::Hyper_L
            | gdk::Key::Hyper_R
            | gdk::Key::Shift_L
            | gdk::Key::Shift_R
            | gdk::Key::ISO_Level3_Shift
    );
    !modifier && (state.intersects(CHORD) || NAVIGATION.contains(&key))
}

/// The column keys of a view; `Stop` when column mode handled the key.
pub fn column_key(view: &StetView, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
    if view.rect().is_none() {
        return glib::Propagation::Proceed;
    }
    let mods = state & gtk::accelerator_get_default_mod_mask();
    let plain = mods.is_empty();
    match key {
        gdk::Key::Escape if plain => {
            view.end_column_mode(Collapse::Cursor);
            glib::Propagation::Stop
        }
        gdk::Key::Left
        | gdk::Key::Right
        | gdk::Key::Up
        | gdk::Key::Down
        | gdk::Key::KP_Left
        | gdk::Key::KP_Right
        | gdk::Key::KP_Up
        | gdk::Key::KP_Down
            if plain =>
        {
            view.end_column_mode(Collapse::Cursor);
            glib::Propagation::Stop
        }
        gdk::Key::Tab | gdk::Key::KP_Tab if plain => {
            view.column_tab();
            glib::Propagation::Stop
        }
        // Shift+Tab unindents the rectangle's lines, as on a stream selection.
        gdk::Key::ISO_Left_Tab => {
            view.end_column_mode(Collapse::Stream);
            glib::Propagation::Proceed
        }
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if !mods.intersects(CHORD) => {
            view.end_column_mode(Collapse::Cursor);
            glib::Propagation::Proceed
        }
        _ => glib::Propagation::Proceed,
    }
}

/// Adds column mode's controllers to a new view.
pub(super) fn connect(view: &StetView) {
    let keys = gtk::EventControllerKey::new();
    keys.set_name(Some(VIEW_KEYS));
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        view,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, state| column_key(&view, key, state)
    ));
    // Added after GtkSourceView's own capture-phase key controller, so it runs before it.
    view.add_controller(keys.clone());
    view.set_column_keys(keys);

    let drag = gtk::GestureDrag::new();
    drag.set_name(Some("stet-column-drag"));
    drag.set_button(gdk::BUTTON_PRIMARY);
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    drag.connect_drag_begin(glib::clone!(
        #[weak]
        view,
        move |gesture, x, y| {
            let state = gesture.current_event_state();
            let alt = state.contains(gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            let claimed = view.pointer_begin(x, y, alt, shift);
            gesture.set_state(if claimed {
                gtk::EventSequenceState::Claimed
            } else {
                gtk::EventSequenceState::Denied
            });
        }
    ));
    drag.connect_drag_update(glib::clone!(
        #[weak]
        view,
        move |gesture, dx, dy| {
            if let Some((x, y)) = gesture.start_point() {
                view.pointer_update(x + dx, y + dy);
            }
        }
    ));
    drag.connect_drag_end(glib::clone!(
        #[weak]
        view,
        move |_, _, _| view.pointer_end()
    ));
    view.add_controller(drag);

    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(glib::clone!(
        #[weak]
        view,
        move |_| view.close_burst()
    ));
    view.add_controller(focus);

    view.connect_realize(|view| {
        if let Some(window) = view.root().and_downcast::<gtk::Window>() {
            hook_window(&window);
        }
    });
}

/// Adds the burst-closing controller to `window` once. Added after the application's
/// accelerator controller, it runs before it.
fn hook_window(window: &gtk::Window) {
    let known = HOOKED.with(|hooked| {
        let mut hooked = hooked.borrow_mut();
        hooked.retain(|weak| weak.upgrade().is_some());
        if hooked
            .iter()
            .any(|weak| weak.upgrade().as_ref() == Some(window))
        {
            return true;
        }
        hooked.push(window.downgrade());
        false
    });
    if known {
        return;
    }
    let events = gtk::EventControllerLegacy::new();
    events.set_name(Some(WINDOW_EVENTS));
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    events.connect_event(|_, event| {
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => close_open_burst(),
            gdk::EventType::KeyPress => {
                if let Some(key) = event.downcast_ref::<gdk::KeyEvent>()
                    && closes_burst(key.keyval(), key.modifier_state())
                {
                    close_open_burst();
                }
            }
            _ => {}
        }
        glib::Propagation::Proceed
    });
    window.add_controller(events);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_navigation_and_escape_close_a_burst() {
        let none = gdk::ModifierType::empty();
        assert!(closes_burst(gdk::Key::z, gdk::ModifierType::CONTROL_MASK));
        assert!(closes_burst(gdk::Key::Down, none));
        assert!(closes_burst(
            gdk::Key::Down,
            gdk::ModifierType::ALT_MASK | gdk::ModifierType::SHIFT_MASK
        ));
        assert!(closes_burst(gdk::Key::Escape, none));
        assert!(closes_burst(gdk::Key::c, gdk::ModifierType::SUPER_MASK));
        assert!(!closes_burst(gdk::Key::a, none));
        assert!(!closes_burst(gdk::Key::A, gdk::ModifierType::SHIFT_MASK));
        assert!(!closes_burst(gdk::Key::BackSpace, none));
        assert!(!closes_burst(gdk::Key::Tab, none));
        assert!(!closes_burst(gdk::Key::braceleft, none));
        assert!(!closes_burst(gdk::Key::Control_L, none));
        assert!(!closes_burst(
            gdk::Key::Alt_L,
            gdk::ModifierType::CONTROL_MASK
        ));
    }
}
