//! Renders the window, and any open popover at its place, to a PNG with GSK's Cairo
//! renderer. Under Broadway this is the only way to see what the app draws; it is what S3 used
//! for its pixel checks.

use crate::convert::{to_f32, to_usize};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, graphene, gsk};
use libadwaita as adw;
use std::path::Path;

pub fn capture(
    window: &adw::ApplicationWindow,
    popovers: &[&gtk::Popover],
    path: &Path,
) -> Result<(), String> {
    let (width, height) = (window.width(), window.height());
    if width <= 0 || height <= 0 {
        return Err("the window has no size yet".to_owned());
    }
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(window)).snapshot(
        &snapshot,
        f64::from(width),
        f64::from(height),
    );
    let (window_x, window_y) = window.surface_transform();
    for popover in popovers.iter().filter(|popover| popover.is_visible()) {
        // A submenu's popup is placed relative to its parent menu's, that one's to the window.
        let (mut popup_x, mut popup_y, mut popups) = (0, 0, 0);
        let mut surface = popover.surface();
        while let Some(popup) = surface.and_then(|surface| surface.downcast::<gdk::Popup>().ok()) {
            popup_x += popup.position_x();
            popup_y += popup.position_y();
            popups += 1;
            surface = popup.parent();
        }
        if popups == 0 {
            continue;
        }
        let (inner_x, inner_y) = popover.surface_transform();
        let x = f64::from(popup_x) + inner_x - window_x;
        let y = f64::from(popup_y) + inner_y - window_y;
        snapshot.save();
        snapshot.translate(&graphene::Point::new(to_f32(x), to_f32(y)));
        gtk::WidgetPaintable::new(Some(*popover)).snapshot(
            &snapshot,
            f64::from(popover.width()),
            f64::from(popover.height()),
        );
        snapshot.restore();
    }
    let node = snapshot
        .to_node()
        .ok_or_else(|| "the window drew nothing".to_owned())?;
    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize_for_display(&WidgetExt::display(window))
        .map_err(|error| error.to_string())?;
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    renderer.unrealize();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    texture.save_to_png(path).map_err(|error| error.to_string())
}

/// The colour `widget` draws at `x`, `y` (widget coordinates), rendered as for a screenshot:
/// red, green, blue and alpha bytes.
pub fn pixel(widget: &gtk::Widget, x: i32, y: i32) -> Result<[u8; 4], String> {
    let (width, height) = (widget.width(), widget.height());
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return Err(format!("{x}, {y} is outside the {width}×{height} widget"));
    }
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(widget)).snapshot(
        &snapshot,
        f64::from(width),
        f64::from(height),
    );
    let node = snapshot
        .to_node()
        .ok_or_else(|| "the widget drew nothing".to_owned())?;
    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize_for_display(&WidgetExt::display(widget))
        .map_err(|error| error.to_string())?;
    let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&viewport));
    renderer.unrealize();
    let stride = to_usize(texture.width()) * 4;
    let mut data = vec![0; stride * to_usize(texture.height())];
    texture.download(&mut data, stride);
    // Cairo's ARGB32: one native-endian word per pixel.
    let at = to_usize(y) * stride + to_usize(x) * 4;
    let [alpha, red, green, blue] =
        u32::from_ne_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]).to_be_bytes();
    Ok([red, green, blue, alpha])
}
