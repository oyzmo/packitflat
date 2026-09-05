// Pack It Flat — GTK4 + libadwaita front end.
//
// This file does application setup only. Everything that decides anything is in
// the library (packitflat::manifest, ::detect, ::project); everything that puts
// it on screen is in window.rs.

#[cfg(debug_assertions)]
mod devshot;
mod build_page;
mod clone_page;
mod editor;
mod forms;
mod panes;
mod proc;
mod style;
mod window;
mod wizard;

use adw::prelude::*;
use gtk::{gio, glib};
use packitflat::i18n::t;
use packitflat::{APP_ID, APP_NAME, VERSION};

use window::PifWindow;

fn main() -> glib::ExitCode {
    // A source-tree run has no installed GSettings schema; build.rs compiled one
    // into OUT_DIR, so point GLib at it before anything asks for a setting.
    #[cfg(debug_assertions)]
    std::env::set_var(
        "GSETTINGS_SCHEMA_DIR",
        concat!(env!("OUT_DIR"), "/schemas"),
    );

    gio::resources_register_include!("packitflat.gresource")
        .expect("the resource bundle is compiled into the binary by build.rs");

    // HANDLES_OPEN so `packitflat some-manifest.yml` and "Open with" from the
    // file manager both work; the .desktop entry advertises the same.
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|app| {
        // The icon lives in the resource bundle, laid out as an icon theme
        // directory, so it resolves by name with nothing installed.
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::IconTheme::for_display(&display)
                .add_resource_path("/no/oyzmo/PackItFlat/icons");
        }
        // GtkBuilder looks types up by name, so GtkSourceView has to be
        // registered before the editor's .ui file is parsed.
        sourceview::View::static_type();
        style::install();
        setup_actions(app);
    });

    app.connect_activate(|app| {
        let window = PifWindow::new(app);
        window.present();
        // The dev hooks belong here too: the dialogs and the template list are
        // reachable without opening a project.
        dev_hooks(&window);
        dev_shot(&window);
    });

    app.connect_open(|app, files, _hint| {
        let window = PifWindow::new(app);
        window.present();
        // Only the first one: this app edits a single project at a time, and
        // silently ignoring the rest would be worse than the window saying so.
        if let Some(path) = files.first().and_then(|f| f.path()) {
            window.open_path(&path);
        }
        dev_hooks(&window);
        dev_shot(&window);
    });

    app.run()
}

/// Nothing at all in a release build; see devshot.rs.
#[cfg(debug_assertions)]
fn dev_shot(window: &PifWindow) {
    if let Some(path) = std::env::var_os(devshot::ENV_VAR) {
        devshot::capture_then_quit(
            window.upcast_ref(),
            path.to_string_lossy().into_owned(),
        );
    }
}

#[cfg(not(debug_assertions))]
fn dev_shot(_window: &PifWindow) {}

/// `PACKITFLAT_DEV_WIZARD=3` opens the wizard at that step, so the screenshot
/// harness can photograph a page nobody has clicked through to.
#[cfg(debug_assertions)]
fn dev_hooks(window: &PifWindow) {
    if let Some(step) = std::env::var("PACKITFLAT_DEV_WIZARD")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        window.dev_open_wizard(step.saturating_sub(1));
    }
    // PACKITFLAT_DEV_BUILD=ready|run|ok|fail|check opens the build page, with a
    // stand-in build where the mode asks for one.
    if let Ok(mode) = std::env::var("PACKITFLAT_DEV_BUILD") {
        window.dev_open_build(mode.trim());
    }

    // PACKITFLAT_DEV_SWITCH=1 types into the guided steps, switches to the
    // editor, and checks that nothing was lost on the way.
    if std::env::var_os("PACKITFLAT_DEV_SWITCH").is_some() {
        window.dev_switch_check();
    }

    // PACKITFLAT_DEV_AUTOSAVE=1 types into a step, presses nothing, and checks
    // that the saved copy caught up by itself.
    if std::env::var_os("PACKITFLAT_DEV_AUTOSAVE").is_some() {
        window.dev_autosave_check();
    }

    // PACKITFLAT_DEV_SCROLL=end scrolls the page down, for photographing what
    // sits below the fold.
    if std::env::var("PACKITFLAT_DEV_SCROLL").as_deref() == Ok("end") {
        window.dev_scroll_to_end();
    }

    // PACKITFLAT_DEV_WRITE=1 writes the files and shows what comes after.
    if std::env::var_os("PACKITFLAT_DEV_WRITE").is_some() {
        window.dev_write_files();
    }

    // PACKITFLAT_DEV_LICENCE=<query> opens the licence picker, types that, and
    // reports what the list is left showing.
    if let Ok(query) = std::env::var("PACKITFLAT_DEV_LICENCE") {
        window.dev_licence_check(query.trim());
    }

    // PACKITFLAT_DEV_CLONE=<url> fetches a repository into PACKITFLAT_DEV_INTO
    // and reports what the app made of it.
    if let Ok(url) = std::env::var("PACKITFLAT_DEV_CLONE") {
        let into = std::env::var("PACKITFLAT_DEV_INTO").unwrap_or_else(|_| "/tmp/clone".into());
        window.dev_clone(url.trim(), std::path::Path::new(&into));
    }

    // PACKITFLAT_DEV_DIALOG=mode|preferences shows one of the dialogs.
    if let Ok(which) = std::env::var("PACKITFLAT_DEV_DIALOG") {
        window.dev_show_dialog(which.trim());
    }

    // PACKITFLAT_DEV_EDITOR=yaml opens the editor showing that pane; adding
    // PACKITFLAT_DEV_SYNCTEST=1 drives the raw pane and reports what happened.
    if let Ok(pane) = std::env::var("PACKITFLAT_DEV_EDITOR") {
        let editor = window.dev_open_editor(pane.trim());
        if std::env::var_os("PACKITFLAT_DEV_SYNCTEST").is_some() {
            let passed = editor.dev_sync_check();
            window.close();
            if !passed {
                std::process::exit(1);
            }
        }
    }

    // PACKITFLAT_DEV_NARROW=360 asks the page on top how narrow it could be, and
    // fails if that is wider than a phone. Before PACKITFLAT_DEV_EXPAND, because
    // an opened explanation is not the state the question is about.
    if let Ok(target) = std::env::var("PACKITFLAT_DEV_NARROW") {
        window.dev_narrow_check(target.trim().parse().unwrap_or(360));
    }

    // PACKITFLAT_DEV_EXPAND=1 opens the first explanation and checks that the
    // window scrolled to show all of it. Last, so that it acts on whatever page
    // the hooks above have opened: it used to run before the wizard and the
    // editor were pushed, so pairing it with PACKITFLAT_DEV_EDITOR reported
    // "there is no expander on this page" about the welcome page.
    if std::env::var_os("PACKITFLAT_DEV_EXPAND").is_some() {
        window.dev_expander_check();
    }
}

#[cfg(not(debug_assertions))]
fn dev_hooks(_window: &PifWindow) {}

fn setup_actions(app: &adw::Application) {
    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| {
            let dialog = adw::AboutDialog::builder()
                .application_name(APP_NAME)
                .application_icon(APP_ID)
                .version(VERSION)
                .developer_name("oyzmo")
                .website("http://oyzmo.no")
                .license_type(gtk::License::Gpl30)
                .comments(t(
                    "Makes a Flatpak out of a program on your computer, without \
                     asking you to learn what a manifest is first.",
                ))
                .build();
            dialog.present(app.active_window().as_ref());
        }
    ));
    app.add_action(&about);

    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| app.quit()
    ));
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<primary>q"]);
}
