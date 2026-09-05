//! Development-only window capture.
//!
//! GNOME refuses non-interactive screenshots over D-Bus, so the window paints
//! itself: GTK already has a renderer attached to the surface, and a
//! `WidgetPaintable` hands it the whole window as a render node. Set
//! `PACKITFLAT_DEV_SHOT` to a path and the app writes that PNG and quits.
//!
//! The whole module is behind `debug_assertions`, so none of it exists in the
//! release build that goes into the Flatpak.

use adw::prelude::*;
use gtk::glib;
use std::time::Duration;

pub const ENV_VAR: &str = "PACKITFLAT_DEV_SHOT";
/// `PACKITFLAT_DEV_SIZE=360x780` resizes before capturing, which is how the
/// 360px-wide phone layout gets checked without a phone.
pub const SIZE_VAR: &str = "PACKITFLAT_DEV_SIZE";

pub fn capture_then_quit(window: &adw::ApplicationWindow, path: String) {
    let window = window.clone();

    if let Some((w, h)) = std::env::var(SIZE_VAR).ok().and_then(|s| parse_size(&s)) {
        window.set_default_size(w, h);
    }

    // Animations would otherwise be caught halfway through, which looks like a
    // layout bug in the picture and isn't one.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
    // Long enough for the first frame, the icon and any list rows to settle.
    glib::timeout_add_local_once(Duration::from_millis(1500), move || {
        if let Ok(depth) = std::env::var("PACKITFLAT_DEV_GEOMETRY") {
            // `=1` is the usual "just show me the tree"; a number deeper than
            // that is how you reach the widgets that are actually overlapping,
            // which are never near the top.
            let deepest = depth
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|d| *d > 1)
                .unwrap_or(10);
            dump_geometry(window.upcast_ref(), 0, deepest);
        }
        match capture(&window, &path) {
            Ok(()) => eprintln!("devshot: wrote {path}"),
            Err(e) => eprintln!("devshot: {e}"),
        }
        // `close()` on its own is refused while a dialog is presented over the
        // window, so photographing any dialog used to leave the harness hanging
        // until whatever timeout was wrapped around it. The application is what
        // has to be told to stop.
        window.close();
        if let Some(app) = window.application() {
            app.quit();
        }
    });
}

/// `PACKITFLAT_DEV_GEOMETRY=1` prints the widget tree's allocations before the
/// snapshot; `=20` goes that many levels deep. Layout questions get answered
/// with numbers rather than by squinting at a picture.
///
/// Each line also carries the **minimum** width the widget demands, which is the
/// number that answers "why won't this window get any narrower". An allocation
/// cannot answer that: a widget squeezed to 341px and a widget that insists on
/// 341px look identical once laid out. Follow `min=` down the tree and the chain
/// where it stops shrinking is the widget doing it.
pub fn dump_geometry(widget: &gtk::Widget, depth: usize, deepest: usize) {
    // Position relative to the window, which is what "is this page where I think
    // it is" questions are actually about.
    let (x, y) = widget
        .root()
        .and_downcast::<gtk::Window>()
        .and_then(|window| widget.compute_bounds(&window))
        .map(|bounds| (bounds.x() as i32, bounds.y() as i32))
        .unwrap_or((0, 0));
    let (min, nat, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
    eprintln!(
        "{:indent$}{} {}x{} at {x},{y} min={min} nat={nat}",
        "",
        widget.type_().name(),
        widget.width(),
        widget.height(),
        indent = depth * 2
    );
    if depth >= deepest {
        return;
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        dump_geometry(&widget, depth + 1, deepest);
        child = widget.next_sibling();
    }
}

fn parse_size(spec: &str) -> Option<(i32, i32)> {
    let (w, h) = spec.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn capture(window: &adw::ApplicationWindow, path: &str) -> Result<(), String> {
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);

    let node = snapshot
        .to_node()
        .ok_or("the window produced an empty render node")?;
    let renderer = window
        .native()
        .and_then(|native| native.renderer())
        .ok_or("the window has no renderer yet")?;

    renderer
        .render_texture(&node, None)
        .save_to_png(path)
        .map_err(|e| format!("could not save {path}: {e}"))
}
