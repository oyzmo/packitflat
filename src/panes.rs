//! The two panes that would otherwise be written twice: what the app is allowed
//! to do, and how it looks in a menu and a store.
//!
//! Both are built into a plain vertical box, which the guided steps and the
//! editor each supply, so the two modes cannot drift into asking different
//! questions or explaining the same switch differently.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;

use packitflat::i18n::t;
use packitflat::permissions::{Level, Permissions, FILESYSTEM_PRESETS, GROUPS};
use packitflat::project::Screenshot;
use packitflat::{appdata, icons, oars, vendor};

use crate::forms::{self, Handle};

/// Read the permissions, change them, and write them back as `finish-args`.
/// Everything the model doesn't recognise is carried through untouched.
fn edit_permissions(handle: &Handle, edit: impl FnOnce(&mut Permissions)) {
    handle.write(|project| {
        let mut permissions = Permissions::parse(&project.manifest.finish_args);
        edit(&mut permissions);
        project.manifest.finish_args = permissions.to_args();
    });
}

fn permissions_of(handle: &Handle) -> Permissions {
    handle.read(|project| Permissions::parse(&project.manifest.finish_args))
}

/// Fill a box with the permissions form. Called again whenever the shape changes
/// — a place added or removed — because the rows capture positions.
pub fn build_permissions(container: &gtk::Box, handle: &Handle, rebuild: Rc<dyn Fn()>) {
    clear_box(container);
    let permissions = permissions_of(handle);

    // What it all adds up to, first: someone should be able to read this and
    // stop, without going through the switches at all.
    let summary = adw::PreferencesGroup::builder()
        .title(t("What this app would be able to do"))
        .description(t(&permissions.summary()))
        .build();

    let risks = permissions.risks();
    let list = boxed_list();
    if risks.is_empty() {
        let row = adw::ActionRow::builder()
            .title(t("Nothing beyond drawing its own window."))
            .subtitle(t(
                "It cannot reach your files, the internet, or anything else on the \
                 computer. This is the best place to be.",
            ))
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
        list.append(&row);
    } else {
        for risk in &risks {
            let detail = match &risk.instead {
                Some(instead) => format!("{}\n\n{instead}", risk.detail),
                None => risk.detail.clone(),
            };
            let row = adw::ActionRow::builder()
                .title(&risk.headline)
                .subtitle(detail)
                .title_lines(0)
                .subtitle_lines(0)
                .build();
            let (icon, css) = match risk.level {
                Level::Serious => ("dialog-warning-symbolic", "note-warning"),
                Level::Notable => ("dialog-information-symbolic", "note-info"),
                Level::Low => ("emblem-important-symbolic", "note-info"),
            };
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            row.add_css_class(css);
            list.append(&row);
        }
    }
    summary.add(&list);
    container.append(&summary);

    // The switches, grouped the way the brief asks: what it does, and what stops
    // working without it.
    for group in GROUPS {
        let widget = adw::PreferencesGroup::builder()
            .title(t(group.title))
            .description(t(group.description))
            .build();
        let list = boxed_list();

        for key in group.keys {
            let key = *key;
            let row = adw::SwitchRow::builder()
                .title(t(key.label()))
                .subtitle(format!(
                    "{}\n{} {}",
                    t(key.explanation()),
                    t("Without it:"),
                    t(key.consequence())
                ))
                .subtitle_lines(0)
                .active(permissions.get(key))
                .build();
            row.connect_active_notify(clone!(
                #[strong]
                handle,
                #[strong]
                rebuild,
                move |row| {
                    let on = row.is_active();
                    edit_permissions(&handle, |permissions| permissions.set(key, on));
                    rebuild();
                }
            ));
            list.append(&row);
        }
        widget.add(&list);
        container.append(&widget);
    }

    container.append(&places_group(handle, &permissions, rebuild.clone()));
    container.append(&services_group(handle, &permissions, rebuild.clone()));
    container.append(&forms::explainer(
        &t("What is this page for?"),
        &t("A Flatpak app starts out able to do almost nothing: it can draw its own \
            window and read its own files, and that is all. Every switch here hands \
            it one more thing — a folder, the internet, your microphone — and it \
            keeps that for good, without ever asking the person using it. Leave off \
            anything the app doesn't genuinely need. For opening and saving files \
            there is usually no need at all: when the app asks for a file the normal \
            way, the system shows the file chooser and hands over only what was \
            picked."),
    ));
    forms::hook_expanders(container);
}

/// Folders the app may reach without asking.
fn places_group(
    handle: &Handle,
    permissions: &Permissions,
    rebuild: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("Files it can reach without asking"))
        .description(t(
            "Most apps need nothing here. When someone picks a file in a file chooser, \
             the app is handed that file whatever this says.",
        ))
        .build();

    let menu = gtk::gio::Menu::new();
    for (id, label, _) in FILESYSTEM_PRESETS {
        menu.append(Some(&t(label)), Some(&format!("permissions.add-place::{id}")));
    }
    menu.append(
        Some(&t("A particular folder…")),
        Some("permissions.choose-place"),
    );

    let add = gtk::MenuButton::builder()
        .valign(gtk::Align::Center)
        .label(t("Add"))
        .menu_model(&menu)
        .build();
    add.add_css_class("flat");
    group.set_header_suffix(Some(&add));

    let list = boxed_list();
    if permissions.filesystems.is_empty() {
        let row = adw::ActionRow::builder()
            .title(t("Nothing — and that's the safest answer"))
            .subtitle(t(
                "The app sees only the files someone hands it through a file chooser.",
            ))
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
        list.append(&row);
    } else {
        for (index, value) in permissions.filesystems.iter().enumerate() {
            let label = FILESYSTEM_PRESETS
                .iter()
                .find(|(id, _, _)| value.split(':').next() == Some(*id))
                .map(|(_, label, description)| (t(label), t(description)))
                .unwrap_or_else(|| {
                    (
                        value.clone(),
                        t("A particular place, which is the careful way to do this."),
                    )
                });

            let row = adw::ActionRow::builder()
                .title(label.0)
                .subtitle(format!("{}\n{value}", label.1))
                .subtitle_lines(0)
                .build();

            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(t("Take this access away"))
                .build();
            remove.add_css_class("flat");
            remove.connect_clicked(clone!(
                #[strong]
                handle,
                #[strong]
                rebuild,
                move |_| {
                    edit_permissions(&handle, |permissions| {
                        permissions.remove_filesystem(index)
                    });
                    rebuild();
                }
            ));
            row.add_suffix(&remove);
            list.append(&row);
        }
    }

    group.add(&list);
    group
}

/// D-Bus services. Named rather than explained one by one: the names are the
/// app's own business, but talking to Flatpak itself is called out by the risk
/// summary above.
fn services_group(
    handle: &Handle,
    permissions: &Permissions,
    rebuild: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("Services it can talk to"))
        .description(t(
            "Other programs on the computer, named the way the system names them. \
             Leave this empty unless something the app uses says otherwise.",
        ))
        .build();

    let add = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .label(t("Add"))
        .build();
    add.add_css_class("flat");
    add.connect_clicked(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_| {
            edit_permissions(&handle, |permissions| {
                permissions.talk_names.push(String::new())
            });
            rebuild();
        }
    ));
    group.set_header_suffix(Some(&add));

    let list = boxed_list();
    if permissions.talk_names.is_empty() {
        list.append(
            &adw::ActionRow::builder()
                .title(t("None"))
                .subtitle(t("The app keeps to itself."))
                .subtitle_lines(0)
                .build(),
        );
    } else {
        for (index, name) in permissions.talk_names.iter().enumerate() {
            let handle_for_entry = handle.clone();
            let (row, _) = forms::entry_row(
                &t("Service"),
                &t("A name like org.freedesktop.Notifications."),
                "org.freedesktop.Notifications",
                name,
                move |value| {
                    edit_permissions(&handle_for_entry, |permissions| {
                        if let Some(slot) = permissions.talk_names.get_mut(index) {
                            *slot = value.trim().to_string();
                        }
                    });
                },
            );

            let remove = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(t("Remove this"))
                .build();
            remove.add_css_class("flat");
            remove.connect_clicked(clone!(
                #[strong]
                handle,
                #[strong]
                rebuild,
                move |_| {
                    edit_permissions(&handle, |permissions| {
                        if index < permissions.talk_names.len() {
                            permissions.talk_names.remove(index);
                        }
                    });
                    rebuild();
                }
            ));
            row.add_suffix(&remove);
            list.append(&row);
        }
    }

    group.add(&list);
    group
}

/// The actions behind the "Add" menu in the places group. Installed by whoever
/// owns the pane, because the folder chooser needs a window to sit over.
pub fn install_permission_actions(
    widget: &impl IsA<gtk::Widget>,
    handle: &Handle,
    rebuild: Rc<dyn Fn()>,
) {
    let actions = gtk::gio::SimpleActionGroup::new();

    let add = gtk::gio::SimpleAction::new("add-place", Some(glib_string_type()));
    add.connect_activate(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_, parameter| {
            let Some(place) = parameter.and_then(|value| value.str().map(str::to_string)) else {
                return;
            };
            edit_permissions(&handle, |permissions| permissions.add_filesystem(&place));
            rebuild();
        }
    ));
    actions.add_action(&add);

    let choose = gtk::gio::SimpleAction::new("choose-place", None);
    let widget_for_choose: gtk::Widget = widget.as_ref().clone();
    choose.connect_activate(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_, _| {
            let dialog = gtk::FileDialog::builder()
                .title(t("Choose a folder the app may always reach"))
                .modal(true)
                .build();
            let window = widget_for_choose
                .root()
                .and_downcast::<gtk::Window>();
            let handle = handle.clone();
            let rebuild = rebuild.clone();
            gtk::glib::spawn_future_local(async move {
                let Ok(file) = dialog.select_folder_future(window.as_ref()).await else {
                    return;
                };
                let Some(path) = file.path() else { return };
                // Written the way flatpak does: under the user's home it becomes
                // ~/… so the manifest works for whoever installs it.
                let place = match gtk::glib::home_dir() {
                    home if path.starts_with(&home) => path
                        .strip_prefix(&home)
                        .map(|rest| format!("~/{}", rest.display()))
                        .unwrap_or_else(|_| path.display().to_string()),
                    _ => path.display().to_string(),
                };
                edit_permissions(&handle, |permissions| permissions.add_filesystem(&place));
                rebuild();
            });
        }
    ));
    actions.add_action(&choose);

    widget.as_ref().insert_action_group("permissions", Some(&actions));
}

fn glib_string_type() -> &'static gtk::glib::VariantTy {
    gtk::glib::VariantTy::STRING
}

// -- dependencies, and building without a network ----------------------------

/// Fill a box with the offline-dependencies form: what this project downloads
/// while building, and how to write that down so the build doesn't have to.
pub fn build_dependencies(container: &gtk::Box, handle: &Handle, rebuild: Rc<dyn Fn()>) {
    clear_box(container);

    let intro = adw::PreferencesGroup::builder()
        .title(t("Building without a network"))
        .description(t(
            "A Flatpak build has no internet access. That is deliberate: it is what makes \
             a build repeatable, and stops a package changing underneath you. Anything \
             your project would download while building has to be written down first — \
             every address and every checksum — in a list the build reads instead.",
        ))
        .build();
    container.append(&intro);

    let (kind, folder, needs) = handle.read(|project| {
        let kind = project
            .source_dir
            .as_deref()
            .map(packitflat::detect::detect)
            .map(|detection| detection.kind)
            .unwrap_or(packitflat::detect::ProjectKind::Unknown);
        (
            kind,
            project.source_dir.clone(),
            vendor::needs(kind, project.source_dir.as_deref(), &project.manifest),
        )
    });

    // A CMake project that fetches its own dependencies has plenty to prepare,
    // so this group is worked out first: "nothing to prepare" must not sit
    // directly above a list of things to prepare.
    let cmake = cmake_group(folder.as_deref());

    if needs.is_empty() && cmake.is_none() {
        let group = adw::PreferencesGroup::builder()
            .title(t("Nothing to prepare"))
            .build();
        let row = adw::ActionRow::builder()
            .title(match kind {
                packitflat::detect::ProjectKind::Unknown => {
                    t("This app can't tell what this project downloads while building.")
                }
                _ => t("This kind of project doesn't download anything while building."),
            })
            .subtitle(t(
                "If the build later stops complaining that it can't reach something, \
                 come back here — that is exactly this problem.",
            ))
            .title_lines(0)
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
        group.add(&row);
        container.append(&group);
    }

    for need in &needs {
        container.append(&dependency_group(handle, need, folder.clone(), rebuild.clone()));
    }

    if let Some(group) = cmake {
        container.append(&group);
    }

    container.append(&snippets_group(handle, rebuild));
    container.append(&forms::explainer(
        &t("What is this page for?"),
        &t("Most projects fetch something while they are being built — the libraries \
            they are written against, or the packages a language's own tool installs. \
            The build here cannot do that, so each one has to be written down in \
            advance: where it comes from, and a checksum, which is a short line of \
            letters and numbers that proves the file you get is the file that was \
            listed. Where this app can work that list out from your project it \
            offers to write it; where it can't, it gives you the exact command that \
            does, and picks the file up once it exists."),
    ));
    forms::hook_expanders(container);
}

/// C and C++ projects have no lock file, but a CMakeLists.txt that fetches its
/// own dependencies has exactly the same problem — and no tool to solve it, so
/// this says what to do by hand.
fn cmake_group(folder: Option<&std::path::Path>) -> Option<adw::PreferencesGroup> {
    let text = std::fs::read_to_string(folder?.join("CMakeLists.txt")).ok()?;
    let downloads = vendor::cmake_downloads(&text);
    if downloads.is_empty() {
        return None;
    }

    let group = adw::PreferencesGroup::builder()
        .title(t("Dependencies this project downloads itself"))
        .description(t(&vendor::cmake_advice(&downloads)))
        .build();

    let list = boxed_list();
    for download in &downloads {
        let row = adw::ActionRow::builder()
            .title(&download.name)
            .subtitle(if download.fetch_content {
                t("Fetched by FetchContent while CMake configures.")
            } else {
                t("Fetched by ExternalProject while the build runs.")
            })
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
        row.add_css_class("note-info");
        list.append(&row);
    }
    group.add(&list);

    let options = adw::ActionRow::builder()
        .title(t("The build option that stops CMake reaching out"))
        .subtitle(vendor::FETCHCONTENT_OFFLINE)
        .subtitle_lines(0)
        .build();
    options.add_css_class("card");

    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(t("Copy it"))
        .build();
    copy.add_css_class("flat");
    copy.connect_clicked(|button| {
        button.clipboard().set_text(vendor::FETCHCONTENT_OFFLINE);
        button.set_tooltip_text(Some(&t("Copied")));
    });
    options.add_suffix(&copy);
    group.add(&options);

    Some(group)
}

fn dependency_group(
    handle: &Handle,
    need: &vendor::Need,
    folder: Option<std::path::PathBuf>,
    rebuild: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let ecosystem = need.ecosystem;
    let group = adw::PreferencesGroup::builder()
        .title(t(ecosystem.label()))
        .description(t(ecosystem.explanation()))
        .build();

    let state = adw::ActionRow::builder()
        .title(t(need.state()))
        .title_lines(0)
        .build();
    // A list this app can write itself and hasn't is a *problem*: it blocks
    // writing the files, and the page should look like it. One that needs a tool
    // this app hasn't got stays a note — the user cannot act on it from here, so
    // shouting at them would be shouting at the wrong person.
    let blocking = !need.is_ready() && need.lock_path.is_some() && ecosystem.prepared_here();
    state.add_prefix(&gtk::Image::from_icon_name(match (need.is_ready(), blocking) {
        (true, _) => "object-select-symbolic",
        (false, true) => "dialog-warning-symbolic",
        (false, false) => "dialog-information-symbolic",
    }));
    match (need.is_ready(), blocking) {
        (true, _) => {}
        (false, true) => state.add_css_class("note-warning"),
        (false, false) => state.add_css_class("note-info"),
    }
    group.add(&state);

    // The state row above already names the file and says what produces it, so
    // there is nothing left to add here.
    if need.lock_path.is_none() {
        return group;
    }

    if ecosystem.can_generate_here() {
        // Say how big the job is before doing it.
        if let Some(lock_path) = &need.lock_path {
            if let Ok(text) = std::fs::read_to_string(lock_path) {
                match vendor::cargo_crate_count(&text) {
                    Ok(count) => group.add(&note_row(
                        &format!(
                            "{count} {}",
                            t("crates will be written down, with an address and a \
                               checksum each.")
                        ),
                        false,
                    )),
                    Err(err) => group.add(&note_row(&err.friendly(), true)),
                }
            }
        }

        let button = gtk::Button::builder()
            .label(if need.is_ready() {
                t("Do it again")
            } else {
                t("Prepare the list")
            })
            .valign(gtk::Align::Center)
            .build();
        if !need.is_ready() {
            button.add_css_class("suggested-action");
        }

        button.connect_clicked(clone!(
            #[strong]
            handle,
            #[strong]
            rebuild,
            #[strong]
            folder,
            move |button| {
                prepare_cargo(&handle, button, folder.as_deref());
                rebuild();
            }
        ));
        group.set_header_suffix(Some(&button));
        return group;
    }

    // The ones this app can't do itself: the command, ready to paste.
    if let Some(command) = ecosystem.external_command() {
        let row = adw::ActionRow::builder()
            .title(t("Run this in the project folder"))
            .subtitle(&command)
            .subtitle_lines(0)
            .build();
        row.add_css_class("card");

        let copy = gtk::Button::builder()
            .icon_name("edit-copy-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(t("Copy the command"))
            .build();
        copy.add_css_class("flat");
        copy.connect_clicked(clone!(
            #[strong]
            command,
            move |button| {
                button.clipboard().set_text(&command);
                button.set_tooltip_text(Some(&t("Copied")));
            }
        ));
        row.add_suffix(&copy);
        group.add(&row);

        group.add(&note_row(
            &t("It comes with flatpak-builder-tools, which your distribution may package \
                separately."),
            false,
        ));
    }

    // Once the file exists, the manifest still has to point at it.
    if need.sources_path.is_some() && !need.wired_in {
        let button = gtk::Button::builder()
            .label(t("Point the manifest at it"))
            .valign(gtk::Align::Center)
            .build();
        button.add_css_class("suggested-action");
        button.connect_clicked(clone!(
            #[strong]
            handle,
            #[strong]
            rebuild,
            move |_| {
                handle.write(|project| vendor::wire_in(&mut project.manifest, ecosystem));
                rebuild();
            }
        ));
        group.set_header_suffix(Some(&button));
    }

    group
}

/// Write the crate list, and wire it in. Everything it needs is already on this
/// computer, so there is nothing to wait for and no way for it to half-work.
fn prepare_cargo(handle: &Handle, button: &gtk::Button, folder: Option<&std::path::Path>) {
    let Some(folder) = folder else { return };

    match vendor::prepare_cargo(folder) {
        Ok(_) => {
            handle.write(|project| vendor::wire_in(&mut project.manifest, vendor::Ecosystem::Cargo))
        }
        Err(err) => show_problem(
            button,
            &t("The list couldn't be prepared"),
            &err.friendly(),
        ),
    }
}

fn show_problem(widget: &impl IsA<gtk::Widget>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", &t("OK"));
    dialog.present(Some(widget.as_ref()));
}

/// A short library of ready-made modules, for the dependency that isn't in the
/// runtime and has to be built alongside the app.
fn snippets_group(handle: &Handle, rebuild: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("Something else the app needs"))
        .description(t(
            "A library that isn't in the runtime has to be built with your app. These are \
             starting points — each one is added above your app and can be edited in the \
             manifest text.",
        ))
        .build();

    for snippet in vendor::SNIPPETS {
        let row = adw::ActionRow::builder()
            .title(t(snippet.name))
            .subtitle(t(snippet.description))
            .subtitle_lines(0)
            .build();

        let add = gtk::Button::builder()
            .label(t("Add"))
            .valign(gtk::Align::Center)
            .build();
        add.connect_clicked(clone!(
            #[strong]
            handle,
            #[strong]
            rebuild,
            move |button| {
                match packitflat::manifest::parse_str(&format!(
                    "modules:\n  - {}",
                    snippet.yaml.replace('\n', "\n    ")
                )) {
                    Ok(import) => {
                        let extra = import.manifest.modules;
                        handle.write(|project| {
                            // Before the app's own module: a dependency has to be
                            // built first, and the app is last by convention.
                            let mut modules = extra.clone();
                            modules.append(&mut project.manifest.modules);
                            project.manifest.modules = modules;
                        });
                        rebuild();
                    }
                    Err(err) => show_problem(
                        button,
                        &t("That couldn't be added"),
                        &err.friendly(),
                    ),
                }
            }
        ));
        row.add_suffix(&add);
        group.add(&row);
    }

    group
}

// -- appearance and store metadata ------------------------------------------

/// Fill a box with the icon, menu placement, version and content-rating form.
pub fn build_appearance(container: &gtk::Box, handle: &Handle, rebuild: Rc<dyn Fn()>) {
    clear_box(container);

    container.append(&icon_group(handle, rebuild.clone()));
    container.append(&menu_group(handle));
    container.append(&version_group(handle));
    container.append(&screenshots_group(handle, rebuild.clone()));
    container.append(&rating_group(handle, rebuild));
    // Not one of the pages that had nothing, strictly — but its first
    // explanation was "Categories", a row about one field, so the page as a
    // whole was never introduced.
    container.append(&forms::explainer(
        &t("What is this page for?"),
        &t("None of this changes what the app does. It is how the app turns up \
            everywhere else: the icon and the name in the menu, the words someone \
            searches for to find it, and the description, pictures and age rating an \
            app store shows on its page. A build with none of it filled in still \
            works — it just arrives as a nameless entry with a blank icon, which is \
            usually not what people mean to release."),
    ));
    forms::hook_expanders(container);
}

fn icon_group(handle: &Handle, rebuild: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("The app's picture"))
        .description(t(
            "Shown in the menu, in the task switcher and in app stores. An SVG is best \
             — it stays sharp at every size.",
        ))
        .build();

    let source = handle.read(|project| project.icon_source.clone());
    let row = adw::ActionRow::builder()
        .title(match &source {
            Some(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| t("A picture")),
            None => t("No picture chosen"),
        })
        .subtitle(match &source {
            Some(path) => path.display().to_string(),
            None => t("Without one the app shows as a blank square."),
        })
        .subtitle_lines(0)
        .build();

    if let Some(path) = &source {
        let preview = gtk::Image::from_file(path);
        preview.set_pixel_size(48);
        row.add_prefix(&preview);
    }

    let choose = gtk::Button::builder()
        .label(t("Choose…"))
        .valign(gtk::Align::Center)
        .build();
    choose.connect_clicked(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        #[weak]
        row,
        move |_| {
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&t("Pictures")));
            filter.add_pattern("*.svg");
            filter.add_pattern("*.png");
            let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);

            let dialog = gtk::FileDialog::builder()
                .title(t("Choose the app's picture"))
                .modal(true)
                .filters(&filters)
                .default_filter(&filter)
                .build();

            let window = row.root().and_downcast::<gtk::Window>();
            let handle = handle.clone();
            let rebuild = rebuild.clone();
            gtk::glib::spawn_future_local(async move {
                let Ok(file) = dialog.open_future(window.as_ref()).await else {
                    return;
                };
                let Some(path) = file.path() else { return };
                handle.write(|project| project.icon_source = Some(path));
                rebuild();
            });
        }
    ));
    row.add_suffix(&choose);
    group.add(&row);

    // Anything wrong with the picture is said here rather than at build time.
    if let Some(path) = &source {
        match icons::inspect(path) {
            Err(err) => group.add(&note_row(&err.friendly(), true)),
            Ok(kind) => {
                for advice in icons::advice(&kind) {
                    group.add(&note_row(&advice, false));
                }
                let app_id = handle.read(|project| project.manifest.app_id.clone());
                if !app_id.trim().is_empty() {
                    group.add(&note_row(
                        &format!(
                            "{} {}",
                            t("It will be copied into the project as"),
                            icons::target_path(app_id.trim(), &kind).display()
                        ),
                        false,
                    ));
                }
            }
        }
    }

    group
}

fn menu_group(handle: &Handle) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("Where it belongs"))
        .description(t(
            "Which part of the menu the app turns up in, and the words people might \
             search for.",
        ))
        .build();

    let chosen = handle.read(|project| project.categories.clone());
    let expander = adw::ExpanderRow::builder()
        .title(t("Categories"))
        .subtitle(if chosen.is_empty() {
            t("None chosen — the app lands under “Other”")
        } else {
            chosen
                .iter()
                .map(|id| appdata::category_label(id).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .subtitle_lines(0)
        .build();

    for (id, label) in appdata::CATEGORIES {
        let row = adw::SwitchRow::builder()
            .title(t(label))
            .subtitle(*id)
            .active(chosen.iter().any(|chosen| chosen == id))
            .build();
        row.connect_active_notify(clone!(
            #[strong]
            handle,
            move |row| {
                let on = row.is_active();
                handle.write(|project| {
                    let list = &mut project.categories;
                    match (on, list.iter().position(|entry| entry == id)) {
                        (true, None) => list.push((*id).to_string()),
                        (false, Some(index)) => {
                            list.remove(index);
                        }
                        _ => {}
                    }
                });
            }
        ));
        expander.add_row(&row);
    }
    group.add(&expander);

    let keywords = handle.read(|project| project.keywords.join(", "));
    let handle_for_keywords = handle.clone();
    let (row, _) = forms::entry_row(
        &t("Search words"),
        &t("Separated by commas. What someone might type when looking for this."),
        "flatpak, manifest, package",
        &keywords,
        move |value| {
            handle_for_keywords.write(|project| {
                project.keywords = value
                    .split(',')
                    .map(str::trim)
                    .filter(|word| !word.is_empty())
                    .map(str::to_string)
                    .collect();
            });
        },
    );
    group.add(&row);
    group
}

fn version_group(handle: &Handle) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("This release"))
        .description(t(
            "Stores show a version and a date, and won't list an app without them.",
        ))
        .build();

    let (version, date, notes) = handle.read(|project| {
        (
            project.release_version.clone(),
            if project.release_date.trim().is_empty() {
                appdata::today()
            } else {
                project.release_date.clone()
            },
            project.release_notes.clone(),
        )
    });

    let handle_for_version = handle.clone();
    let (version_row, _) = forms::entry_row(
        &t("Version"),
        &t("Numbers separated by dots. 1.0.0 is a fine place to start."),
        "1.0.0",
        &version,
        move |value| {
            handle_for_version.write(|project| project.release_version = value.trim().to_string());
        },
    );
    group.add(&version_row);

    let handle_for_date = handle.clone();
    let (date_row, _) = forms::entry_row(
        &t("Date"),
        &t("Year-month-day. Today's date is filled in for you."),
        "2026-09-04",
        &date,
        move |value| {
            handle_for_date.write(|project| project.release_date = value.trim().to_string());
        },
    );
    group.add(&date_row);
    // Today's date has to reach the project even if nobody edits the field.
    handle.write(|project| {
        if project.release_date.trim().is_empty() {
            project.release_date = date.clone();
        }
    });

    let handle_for_notes = handle.clone();
    let (notes_row, _) = forms::entry_row(
        &t("What changed"),
        &t("One line, shown beside the version in stores."),
        "First public release",
        &notes,
        move |value| {
            handle_for_notes.write(|project| project.release_notes = value);
        },
    );
    group.add(&notes_row);
    group
}

fn screenshots_group(handle: &Handle, rebuild: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(t("Pictures of the app"))
        .description(t(
            "Stores show these on the app's page. They are fetched over the web, so each \
             one is an address rather than a file on this computer.",
        ))
        .build();

    let add = gtk::Button::builder()
        .valign(gtk::Align::Center)
        .label(t("Add"))
        .build();
    add.add_css_class("flat");
    add.connect_clicked(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_| {
            handle.write(|project| {
                project.screenshots.push(Screenshot {
                    url: String::new(),
                    caption: String::new(),
                })
            });
            rebuild();
        }
    ));
    group.set_header_suffix(Some(&add));

    let screenshots = handle.read(|project| project.screenshots.clone());
    if screenshots.is_empty() {
        group.add(&note_row(
            &t("None yet. An app with no pictures looks unfinished in a store, but \
                nothing here stops it from being built."),
            false,
        ));
        return group;
    }

    for (index, shot) in screenshots.iter().enumerate() {
        let row = adw::ExpanderRow::builder()
            .title(if shot.url.trim().is_empty() {
                t("Not filled in yet")
            } else {
                shot.url.clone()
            })
            .subtitle(if index == 0 {
                t("Shown first, and used as the app's main picture")
            } else {
                t("Shown after the others")
            })
            .subtitle_lines(0)
            .expanded(shot.url.trim().is_empty())
            .build();

        let handle_for_url = handle.clone();
        let (url_row, _) = forms::entry_row(
            &t("Address"),
            &t("A link to a PNG, ideally 1600 pixels or so across."),
            "https://example.org/screenshots/main.png",
            &shot.url,
            move |value| {
                handle_for_url.write(|project| {
                    if let Some(shot) = project.screenshots.get_mut(index) {
                        shot.url = value.trim().to_string();
                    }
                });
            },
        );
        row.add_row(&url_row);

        let handle_for_caption = handle.clone();
        let (caption_row, _) = forms::entry_row(
            &t("Caption"),
            &t("One short line saying what the picture shows."),
            "The welcome page",
            &shot.caption,
            move |value| {
                handle_for_caption.write(|project| {
                    if let Some(shot) = project.screenshots.get_mut(index) {
                        shot.caption = value;
                    }
                });
            },
        );
        row.add_row(&caption_row);

        let remove = adw::ActionRow::builder()
            .title(t("Remove this picture"))
            .activatable(true)
            .build();
        remove.add_prefix(&gtk::Image::from_icon_name("user-trash-symbolic"));
        remove.add_css_class("error");
        remove.connect_activated(clone!(
            #[strong]
            handle,
            #[strong]
            rebuild,
            move |_| {
                handle.write(|project| {
                    if index < project.screenshots.len() {
                        project.screenshots.remove(index);
                    }
                });
                rebuild();
            }
        ));
        row.add_row(&remove);
        group.add(&row);
    }

    group
}

fn rating_group(handle: &Handle, rebuild: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let answers = handle.read(|project| project.content_rating.clone());
    let group = adw::PreferencesGroup::builder()
        .title(t("Age rating"))
        .description(t(
            "Stores ask every app these. Answering “No” to all of them — the usual answer \
             for a tool — is itself an answer, and is what gets written.",
        ))
        .build();

    for question in oars::QUESTIONS {
        let labels = gtk::StringList::new(&[]);
        for answer in question.answers {
            labels.append(&t(answer.label));
        }

        let row = adw::ComboRow::builder()
            .title(t(question.text))
            .subtitle(t(question.help))
            .subtitle_lines(0)
            .model(&labels)
            .selected(oars::answer_index(&answers, question.id) as u32)
            .build();

        row.connect_selected_notify(clone!(
            #[strong]
            handle,
            #[strong]
            rebuild,
            move |row| {
                let index = row.selected() as usize;
                handle.write(|project| {
                    oars::set_answer(&mut project.content_rating, question.id, index)
                });
                rebuild();
            }
        ));
        group.add(&row);
    }

    group.add(&note_row(&oars::summary(&answers), false));
    group
}

// -- small shared pieces ------------------------------------------------------

fn note_row(text: &str, bad: bool) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(text)
        .title_lines(0)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(if bad {
        "dialog-warning-symbolic"
    } else {
        "dialog-information-symbolic"
    }));
    row.add_css_class(if bad { "note-warning" } else { "note-info" });
    row
}

fn boxed_list() -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    list
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
