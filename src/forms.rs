//! The parts of the two editing modes that must not drift apart.
//!
//! The guided steps and the editor both let someone say where the code comes
//! from, and there is exactly one implementation of that here: same fields, same
//! explanations, same file choosers, same checksum button. A [`Handle`] gives
//! both of them the same project and the same "something changed" signal, so
//! neither mode holds a copy of anything.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib::clone;
use gtk::glib;

use packitflat::i18n::t;
use packitflat::manifest::{
    BuildOptions, BuildSystem, Manifest, Module, ModuleEntry, Source, SourceEntry, SourceKind,
};
use packitflat::project::{self, Project};
use packitflat::validate::{Issue, Severity};
use packitflat::{generate, runtimes, sha256};

/// Autosaving through a function pointer so `write_manifest` can hand it to
/// `Handle::read` without borrowing twice.
fn project_save(project: &Project) -> anyhow::Result<std::path::PathBuf> {
    project::save(project)
}

/// A shared, writable view of the project plus a way to say it changed.
///
/// `silently` exists for the one moment the flow has to be broken: while widgets
/// are being filled in from the project, their change handlers must not write
/// what they were just given straight back.
#[derive(Clone)]
pub struct Handle {
    project: Rc<RefCell<Project>>,
    changed: Rc<dyn Fn()>,
    guard: Rc<Cell<bool>>,
    /// The autosave waiting to happen, so the next change can call it off. `None`
    /// on a handle that doesn't save — the build page holds one and edits
    /// nothing.
    pending_save: Rc<RefCell<Option<glib::SourceId>>>,
    saving: Rc<Cell<bool>>,
}

// The widget templates that hold a Handle derive Debug; the callback inside
// can't, so the interesting half is printed and the callback is named.
impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("muted", &self.guard.get())
            .field("changed", &"<callback>")
            .finish()
    }
}

impl Handle {
    pub fn new(project: Rc<RefCell<Project>>, changed: impl Fn() + 'static) -> Self {
        Handle {
            project,
            changed: Rc::new(changed),
            guard: Rc::new(Cell::new(false)),
            pending_save: Rc::new(RefCell::new(None)),
            saving: Rc::new(Cell::new(false)),
        }
    }

    /// Autosave a moment after the last change, for a mode that edits.
    ///
    /// The point of the saved copy is surviving an interruption, and it used to
    /// be written only when the page changed or the mode was switched — so a
    /// crash or a power cut lost everything typed on the page you were on, which
    /// is the one case it exists for. A change resets the wait rather than
    /// queueing another save, so typing a sentence writes the file once at the
    /// end of it rather than once per letter.
    pub fn autosave(&self) -> Self {
        self.saving.set(true);
        self.clone()
    }

    fn save_soon(&self) {
        if !self.saving.get() {
            return;
        }
        if let Some(pending) = self.pending_save.borrow_mut().take() {
            pending.remove();
        }

        let handle = self.clone();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
            handle.pending_save.replace(None);
            handle.save_now();
        });
        self.pending_save.replace(Some(id));
    }

    /// Write the saved copy at once, and drop any wait that was still running.
    /// Called where the old code called `save`: leaving a page, switching mode,
    /// writing the files.
    pub fn save_now(&self) {
        if let Some(pending) = self.pending_save.borrow_mut().take() {
            pending.remove();
        }
        if let Err(err) = self.read(project_save) {
            eprintln!("packitflat: could not autosave the project: {err:#}");
        }
    }

    pub fn shared(&self) -> Rc<RefCell<Project>> {
        self.project.clone()
    }

    pub fn read<T>(&self, f: impl FnOnce(&Project) -> T) -> T {
        f(&self.project.borrow())
    }

    /// Change the project and tell everyone. Ignored while filling widgets.
    pub fn write(&self, f: impl FnOnce(&mut Project)) {
        if self.guard.get() {
            return;
        }
        f(&mut self.project.borrow_mut());
        (self.changed)();
        self.save_soon();
    }

    pub fn edit_source(&self, index: usize, f: impl FnOnce(&mut Source)) {
        self.write(|project| {
            let Some(module) = project.manifest.main_module_mut() else {
                return;
            };
            if let Some(SourceEntry::Source(source)) = module.sources.get_mut(index) {
                f(source);
            }
        });
    }

    /// Run something with the change handlers muted.
    pub fn silently<T>(&self, f: impl FnOnce() -> T) -> T {
        let was = self.guard.replace(true);
        let result = f();
        self.guard.set(was);
        result
    }

    pub fn is_muted(&self) -> bool {
        self.guard.get()
    }

    /// Ask for everything derived from the project to be redrawn.
    pub fn notify(&self) {
        if !self.guard.get() {
            (self.changed)();
        }
    }
}

/// A labelled text field: label, one line of plain English, and a realistic
/// example. All three, every time — it is the brief's first rule and this is the
/// only place that builds one.
pub fn entry_row(
    title: &str,
    subtitle: &str,
    placeholder: &str,
    value: &str,
    on_change: impl Fn(String) + 'static,
) -> (adw::ActionRow, gtk::Entry) {
    // `width-chars` sets the *minimum*, and 18 of them is 180px that no window
    // can shrink below. On a row that also carries a button — the source's path,
    // with "Choose…" beside it — that was the floor under the whole editor's
    // width. The natural size is what decides how wide the field looks when
    // there is room, so `max-width-chars` keeps the appearance and the smaller
    // `width-chars` lets it give way when there isn't. `hexpand` already means a
    // wide window fills it out regardless.
    let entry = gtk::Entry::builder()
        .valign(gtk::Align::Center)
        .hexpand(true)
        .width_chars(8)
        .max_width_chars(18)
        .placeholder_text(placeholder)
        .text(value)
        .build();
    entry.connect_changed(move |entry| on_change(entry.text().to_string()));

    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .subtitle_lines(0)
        .build();
    row.add_suffix(&entry);
    (row, entry)
}

/// Fill a list box with one row per source, plus the empty state. `rebuild` is
/// called when the *shape* changes — a source added or removed — because the row
/// closures capture positions.
pub fn fill_sources(
    list: &gtk::ListBox,
    handle: &Handle,
    parent: &impl IsA<gtk::Widget>,
    rebuild: Rc<dyn Fn()>,
) {
    clear(list);

    let sources = handle.read(|project| {
        project
            .manifest
            .main_module()
            .map(|module| module.sources.clone())
            .unwrap_or_default()
    });

    if sources.is_empty() {
        let row = adw::ActionRow::builder()
            .title(t("Nothing here yet"))
            .subtitle(t(
                "Use Add to say where the code comes from — for most projects that's \
                 the folder it lives in.",
            ))
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
        row.add_css_class("note-warning");
        list.append(&row);
        return;
    }

    for (index, entry) in sources.iter().enumerate() {
        match entry {
            SourceEntry::Include(path) => {
                let row = adw::ActionRow::builder()
                    .title(path)
                    .subtitle(t(
                        "A list of extra sources kept in another file — usually the \
                         dependencies prepared for building offline. Left exactly as it is.",
                    ))
                    .subtitle_lines(0)
                    .build();
                row.add_suffix(&remove_button(handle, index, rebuild.clone()));
                list.append(&row);
            }
            SourceEntry::Source(source) => {
                list.append(&source_row(handle, parent, index, source, rebuild.clone()));
            }
        }
    }

    // The source rows are expanders too, and they are built fresh each time.
    hook_expanders(list);
}

/// Add a source of the given kind and rebuild the list around it.
pub fn add_source(handle: &Handle, kind: SourceKind, rebuild: &Rc<dyn Fn()>) {
    handle.write(|project| {
        let name = module_name(project);
        let module = project.manifest.ensure_main_module(&name);
        module.sources.push(SourceEntry::Source(Source {
            kind,
            ..Source::default()
        }));
    });
    rebuild();
}

fn module_name(project: &Project) -> String {
    project
        .manifest
        .main_module()
        .map(|module| module.name.clone())
        .filter(|name| !name.is_empty())
        .or_else(|| {
            project
                .source_dir
                .as_ref()
                .and_then(|dir| dir.file_name())
                .map(|name| name.to_string_lossy().to_lowercase())
        })
        .unwrap_or_else(|| "app".to_string())
}

fn source_row(
    handle: &Handle,
    parent: &impl IsA<gtk::Widget>,
    index: usize,
    source: &Source,
    rebuild: Rc<dyn Fn()>,
) -> adw::ExpanderRow {
    // A concrete widget, because the file-chooser closures need something they
    // can hold a weak reference to.
    let parent: gtk::Widget = parent.as_ref().clone();

    let row = adw::ExpanderRow::builder()
        .title(if source.label().is_empty() {
            t("Not filled in yet")
        } else {
            source.label()
        })
        .subtitle(t(source.kind.explanation()))
        .subtitle_lines(0)
        .expanded(source.label().is_empty())
        .build();

    match source.kind {
        SourceKind::Git => {
            row.add_row(&field(
                handle,
                index,
                t("Repository address"),
                t("The address you would give to “git clone”."),
                "https://github.com/someone/project.git",
                source.url.clone().unwrap_or_default(),
                |source, value| source.url = non_empty(value),
            ));
            row.add_row(&field(
                handle,
                index,
                t("Tag"),
                t("The released version to build, such as v1.2.0. Leave empty if you are \
                   using a commit instead."),
                "v1.2.0",
                source.tag.clone().unwrap_or_default(),
                |source, value| source.tag = non_empty(value),
            ));
            row.add_row(&field(
                handle,
                index,
                t("Commit"),
                t("The exact revision to build. More precise than a tag, because a tag \
                   can be moved."),
                "0a1b2c3d…",
                source.commit.clone().unwrap_or_default(),
                |source, value| source.commit = non_empty(value),
            ));
            row.add_row(&field(
                handle,
                index,
                t("Branch"),
                t("Only if there is no tag or commit. The build then changes whenever \
                   the branch does."),
                "main",
                source.branch.clone().unwrap_or_default(),
                |source, value| source.branch = non_empty(value),
            ));
        }
        SourceKind::Archive => {
            row.add_row(&field(
                handle,
                index,
                t("Download address"),
                t("A link to a .tar.gz, .tar.xz or .zip file."),
                "https://example.org/project-1.2.0.tar.xz",
                source.url.clone().unwrap_or_default(),
                |source, value| source.url = non_empty(value),
            ));

            let hash_row = field(
                handle,
                index,
                t("Checksum"),
                t("Proves the downloaded file is the one you meant. The build stops if \
                   it ever changes."),
                "64 characters of digits and a–f",
                source.sha256.clone().unwrap_or_default(),
                |source, value| source.sha256 = non_empty(value),
            );
            let button = gtk::Button::builder()
                .label(t("From a file…"))
                .valign(gtk::Align::Center)
                .tooltip_text(t(
                    "Work the checksum out from a copy of the file on this computer",
                ))
                .build();
            button.connect_clicked(clone!(
                #[strong]
                handle,
                #[weak(rename_to = parent)]
                parent,
                #[strong]
                rebuild,
                move |_| compute_checksum(&handle, &parent, index, rebuild.clone())
            ));
            hash_row.add_suffix(&button);
            row.add_row(&hash_row);
        }
        _ => {
            let path_row = field(
                handle,
                index,
                t("Path"),
                t("Where it is, relative to the folder holding the manifest. “.” means \
                   that folder itself."),
                ".",
                source.path.clone().unwrap_or_default(),
                |source, value| source.path = non_empty(value),
            );
            let button = gtk::Button::builder()
                .label(t("Choose…"))
                .valign(gtk::Align::Center)
                .build();
            let folder = source.kind == SourceKind::Dir;
            button.connect_clicked(clone!(
                #[strong]
                handle,
                #[weak(rename_to = parent)]
                parent,
                #[strong]
                rebuild,
                move |_| choose_path(&handle, &parent, index, folder, rebuild.clone())
            ));
            path_row.add_suffix(&button);
            row.add_row(&path_row);
        }
    }

    let remove = adw::ActionRow::builder()
        .title(t("Remove this"))
        .activatable(true)
        .build();
    remove.add_prefix(&gtk::Image::from_icon_name("user-trash-symbolic"));
    remove.add_css_class("error");
    remove.connect_activated(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_| remove_source(&handle, index, &rebuild)
    ));
    row.add_row(&remove);

    row
}

fn field(
    handle: &Handle,
    index: usize,
    title: String,
    subtitle: String,
    placeholder: &str,
    value: String,
    apply: fn(&mut Source, String),
) -> adw::ActionRow {
    let handle = handle.clone();
    let (row, _) = entry_row(&title, &subtitle, placeholder, &value, move |value| {
        handle.edit_source(index, |source| apply(source, value));
    });
    row
}

fn remove_button(handle: &Handle, index: usize, rebuild: Rc<dyn Fn()>) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(t("Remove this"))
        .build();
    button.add_css_class("flat");
    button.connect_clicked(clone!(
        #[strong]
        handle,
        #[strong]
        rebuild,
        move |_| remove_source(&handle, index, &rebuild)
    ));
    button
}

fn remove_source(handle: &Handle, index: usize, rebuild: &Rc<dyn Fn()>) {
    handle.write(|project| {
        if let Some(module) = project.manifest.main_module_mut() {
            if index < module.sources.len() {
                module.sources.remove(index);
            }
        }
    });
    rebuild();
}

fn choose_path(
    handle: &Handle,
    parent: &impl IsA<gtk::Widget>,
    index: usize,
    folder: bool,
    rebuild: Rc<dyn Fn()>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(if folder {
            t("Choose the folder holding the code")
        } else {
            t("Choose a file")
        })
        .modal(true)
        .build();

    let handle = handle.clone();
    let window = window_of(parent);
    glib::spawn_future_local(async move {
        let result = if folder {
            dialog.select_folder_future(window.as_ref()).await
        } else {
            dialog.open_future(window.as_ref()).await
        };
        let Ok(file) = result else { return };
        let Some(path) = file.path() else { return };

        // Relative to the project folder when it is inside it: a manifest full of
        // absolute paths only builds on the machine that wrote it.
        let base = handle.read(|project| project.source_dir.clone());
        let text = relative_to(&path, base.as_deref());
        handle.edit_source(index, |source| source.path = Some(text));
        rebuild();
    });
}

/// The checksum, worked out from a copy of the file the user already has.
/// Downloading it would need network permission this app deliberately doesn't
/// ask for; the address is still recorded, and the build checks it later.
fn compute_checksum(
    handle: &Handle,
    parent: &impl IsA<gtk::Widget>,
    index: usize,
    rebuild: Rc<dyn Fn()>,
) {
    let dialog = gtk::FileDialog::builder()
        .title(t("Choose the downloaded archive"))
        .modal(true)
        .build();

    let handle = handle.clone();
    let window = window_of(parent);
    glib::spawn_future_local(async move {
        let Ok(file) = dialog.open_future(window.as_ref()).await else {
            return;
        };
        let Some(path) = file.path() else { return };

        match sha256::hash_file(&path) {
            Ok(hash) => {
                handle.edit_source(index, |source| source.sha256 = Some(hash));
                rebuild();
            }
            Err(err) => {
                if let Some(window) = window {
                    let dialog = adw::AlertDialog::builder()
                        .heading(t("That file couldn't be read"))
                        .body(format!("{}\n\n{err}", path.display()))
                        .build();
                    dialog.add_response("ok", &t("OK"));
                    dialog.present(Some(&window));
                }
            }
        }
    });
}

fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.as_ref().root().and_downcast::<gtk::Window>()
}

/// Write the project's files and say what happened. Shared so both modes write
/// the same files the same way and report them in the same words.
pub fn write_files(
    handle: &Handle,
    parent: &impl IsA<gtk::Widget>,
    plan: &generate::Plan,
    backup: bool,
) {
    let parent: gtk::Widget = parent.as_ref().clone();

    match handle.read(|project| generate::write(project, plan, backup)) {
        Ok(written) => {
            if let Err(err) = handle.read(project_save) {
                eprintln!("packitflat: could not autosave the project: {err:#}");
            }
            show_written(handle, &parent, &plan.folder, &written);
        }
        Err(err) => show_error(
            &parent,
            &t("The files couldn't all be written"),
            &err.friendly(),
        ),
    }
}

/// The everything-at-once version, for the editor's toolbar button: plan and
/// write in one go, with backups on.
pub fn write_manifest(handle: &Handle, parent: &impl IsA<gtk::Widget>, backup: bool) {
    match handle.read(generate::plan) {
        Ok(plan) => write_files(handle, parent, &plan, backup),
        Err(err) => show_error(
            parent,
            &t("The files can't be written yet"),
            &err.friendly(),
        ),
    }
}

fn show_written(
    handle: &Handle,
    parent: &gtk::Widget,
    folder: &Path,
    written: &[generate::Written],
) {
    let handle_for_build = handle.clone();
    let mut body = String::new();
    for file in written {
        body.push_str(&format!(
            "{}\n",
            file.path
                .strip_prefix(folder)
                .unwrap_or(&file.path)
                .display()
        ));
    }

    let kept: Vec<String> = written
        .iter()
        .filter_map(|file| file.backup_path.as_ref())
        .map(|path| {
            path.strip_prefix(folder)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect();
    if !kept.is_empty() {
        body.push_str(&format!(
            "\n{}\n{}\n",
            t("What was there before has been kept as:"),
            kept.join("\n")
        ));
    }

    let dialog = adw::AlertDialog::builder()
        .heading(if written.len() == 1 {
            t("The file is written")
        } else {
            format!("{} files are written", written.len())
        })
        .body(format!(
            "{}\n{body}\n{}",
            folder.display(),
            t("They are ordinary text files, and the manifest says at the top how to \
               build it.")
        ))
        .build();
    dialog.add_response("close", &t("Stay here"));
    dialog.add_response("show", &t("Show the folder"));
    dialog.add_response("home", &t("Back to the start"));
    // The obvious next thing to want, and the reason all this was written.
    dialog.add_response("build", &t("Build it now"));
    dialog.set_response_appearance("build", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("build"));

    let folder = folder.to_path_buf();
    let for_closure = parent.clone();
    dialog.connect_response(None, move |_, response| match response {
        "show" => {
            let uri = gtk::gio::File::for_path(&folder).uri();
            let _ = gtk::gio::AppInfo::launch_default_for_uri(
                &uri,
                gtk::gio::AppLaunchContext::NONE,
            );
        }
        "build" => {
            crate::build_page::PifBuild::push_from(&for_closure, handle_for_build.shared())
        }
        // Back to the welcome page, over whatever pages are stacked up. The
        // project is saved, and it will be waiting under "Pick up where you
        // left off".
        "home" => {
            if let Some(view) = for_closure
                .ancestor(adw::NavigationView::static_type())
                .and_downcast::<adw::NavigationView>()
            {
                view.pop_to_tag("welcome");
            }
        }
        _ => {}
    });
    dialog.present(Some(parent));
}

pub fn show_error(parent: &impl IsA<gtk::Widget>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", &t("OK"));
    dialog.set_default_response(Some("ok"));
    dialog.present(Some(parent.as_ref()));
}

/// A path inside the project folder becomes relative to it; anything else stays
/// absolute, because a wrong relative path is worse than a long one.
pub fn relative_to(path: &Path, base: Option<&Path>) -> String {
    match base.and_then(|base| path.strip_prefix(base).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Some(relative) => relative.display().to_string(),
        None => path.display().to_string(),
    }
}

pub fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

// -- the licence picker ------------------------------------------------------

/// What the licence row says when nothing is chosen yet.
pub fn licence_summary(id: &str) -> String {
    match packitflat::spdx::find(id.trim()) {
        Some(license) => license.id.to_string(),
        None if id.trim().is_empty() => t("Not chosen yet"),
        // A licence typed into the editor, or imported from somebody else's
        // manifest, is shown as it stands rather than being called nothing.
        None => id.trim().to_string(),
    }
}

/// Open the licence list and call `chosen` with the SPDX identifier picked —
/// the empty string for "none yet".
///
/// This is a dialog rather than an `AdwComboRow` with `enable-search`, because
/// that search does not work, in two separate ways that were both measured with
/// `PACKITFLAT_DEV_LICENCE` rather than guessed at:
///
/// - libadwaita clears the popup's search box before what was typed reaches its
///   filter, so the list never narrows however the filter is set up. The signal
///   trace is `changed "gplv3"` then `changed ""` then `search-changed ""`.
/// - The position the row reports back is an index into the *filtered* list,
///   while the list of licences it is looked up in is the unfiltered one. So
///   even with the search fixed, picking a licence after typing stores a
///   different licence — silently, into the metainfo an app store reads.
///
/// Both go away with a list this app builds itself out of `spdx::search`, which
/// is unit-tested and already knows that "gplv3" means GPL-3.0-or-later. It also
/// has room for the sentence saying what each licence actually allows, which is
/// the thing a beginner needs and which a combo row has nowhere to put.
pub fn choose_licence(
    parent: &impl IsA<gtk::Widget>,
    current: &str,
    chosen: impl Fn(&str) + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title(t("Choose a licence"))
        .content_width(620)
        .content_height(620)
        .build();

    let search = gtk::SearchEntry::builder()
        .placeholder_text(t("Search: gplv3, mit, apache…"))
        .hexpand(true)
        .build();

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        // Named so the dev check can find *this* list: an AdwPreferencesGroup
        // has a GtkListBox of its own, and it comes first in tree order.
        .name("licence-list")
        .build();
    list.add_css_class("boxed-list");

    let group = adw::PreferencesGroup::builder()
        .description(t(
            "A licence says what other people may do with your code. If you have \
             no idea, GPL-3.0-or-later is the usual choice for a GNOME app.",
        ))
        .build();
    group.add(&list);

    let page = adw::PreferencesPage::new();
    page.add(&group);

    let nothing = adw::StatusPage::builder()
        .icon_name("system-search-symbolic")
        .title(t("No licence by that name"))
        .description(t(
            "Try part of it — “gpl”, “mit”, “creative commons”. If yours really \
             isn't here, pick “Something else, written by me”.",
        ))
        .visible(false)
        .build();

    let stack = gtk::Stack::new();
    stack.add_named(&page, Some("list"));
    stack.add_named(&nothing, Some("nothing"));

    let current = current.trim().to_string();
    let chosen = Rc::new(chosen);

    // Rebuilt from `spdx::search` on every keystroke. The rows are few enough
    // (thirty, and fewer once anything is typed) that rebuilding is simpler than
    // filtering, and simpler is what went wrong last time.
    let fill = {
        let list = list.clone();
        let stack = stack.clone();
        let dialog = dialog.clone();
        let chosen = chosen.clone();
        let current = current.clone();
        move |query: &str| {
            while let Some(row) = list.first_child() {
                list.remove(&row);
            }

            let matches = packitflat::spdx::search(query);
            stack.set_visible_child_name(if matches.is_empty() { "nothing" } else { "list" });

            // "None yet" belongs with the licences, not off in a corner: it is a
            // real answer, and someone who set one by mistake has to be able to
            // take it back.
            if query.trim().is_empty() {
                let row = adw::ActionRow::builder()
                    .title(t("Not chosen yet"))
                    .subtitle(t(
                        "Leave it blank for now. App stores will not list the app \
                         until it has one.",
                    ))
                    .subtitle_lines(0)
                    .activatable(true)
                    .build();
                if current.is_empty() {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                }
                row.connect_activated(clone!(
                    #[weak]
                    dialog,
                    #[strong]
                    chosen,
                    move |_| {
                        dialog.close();
                        chosen("");
                    }
                ));
                list.append(&row);
            }

            for license in matches {
                let row = adw::ActionRow::builder()
                    .title(format!("{} — {}", license.id, t(license.name)))
                    .subtitle(t(license.summary))
                    .subtitle_lines(0)
                    .title_lines(0)
                    .activatable(true)
                    .build();
                if license.id.eq_ignore_ascii_case(&current) {
                    row.add_suffix(&gtk::Image::from_icon_name("object-select-symbolic"));
                }
                row.connect_activated(clone!(
                    #[weak]
                    dialog,
                    #[strong]
                    chosen,
                    move |_| {
                        dialog.close();
                        chosen(license.id);
                    }
                ));
                list.append(&row);
            }
        }
    };
    fill("");

    search.connect_search_changed(move |entry| fill(&entry.text()));

    // The search box goes under the header rather than replacing its title: a
    // dialog that opens showing only a search field does not say what it is
    // asking, and that is the one thing this app is careful about.
    let search_bar = gtk::Box::builder()
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(6)
        .build();
    search_bar.append(&search);

    let view = adw::ToolbarView::builder().content(&stack).build();
    view.add_top_bar(&adw::HeaderBar::new());
    view.add_top_bar(&search_bar);
    dialog.set_child(Some(&view));
    dialog.present(Some(parent.as_ref()));

    // The point of the dialog is the search box; landing in it saves a click on
    // the one control the whole thing exists for.
    search.grab_focus();
}

/// The first descendant of a type carrying a given widget name. Where several
/// widgets of a type are nested — a list of ours inside a list libadwaita built
/// — the name is the only thing that tells them apart.
///
/// Only the `PACKITFLAT_DEV_*` checks go looking for widgets like this; the app
/// itself holds the ones it needs. Hence the `cfg`: without it a release build
/// warns that it is dead code, because every caller is behind the same one.
#[cfg(debug_assertions)]
pub fn descendant_named<T: IsA<gtk::Widget>>(root: &gtk::Widget, name: &str) -> Option<T> {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Ok(found) = widget.clone().downcast::<T>() {
            if found.as_ref().widget_name() == name {
                return Some(found);
            }
        }
        if let Some(found) = descendant_named::<T>(&widget, name) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// The first descendant of a type, anywhere under `root`. Widgets libadwaita
/// builds for itself — a dialog, its search box, its list — can only be reached
/// by going to look for them. Debug-only, as above.
#[cfg(debug_assertions)]
pub fn descendant<T: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<T> {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Ok(found) = widget.clone().downcast::<T>() {
            return Some(found);
        }
        if let Some(found) = descendant::<T>(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// Make every "What is this?" row in a page scroll itself into view when it is
/// opened. An explanation that unfolds below the bottom of the window is an
/// explanation nobody reads, and these are the rows the whole app is built
/// around.
///
/// Called on a page after it is built, so it catches the rows in the `.ui` file
/// and the ones built in code alike.
pub fn hook_expanders(root: &impl IsA<gtk::Widget>) {
    let mut child = root.as_ref().first_child();
    while let Some(widget) = child {
        if let Some(expander) = widget.downcast_ref::<adw::ExpanderRow>() {
            hook_expander(expander);
        }
        if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
            skip_row_when_tabbing(row);
        }
        hook_expanders(&widget);
        child = widget.next_sibling();
    }
}

/// A row whose whole point is the text box in it shouldn't take a Tab stop of
/// its own — pressing Tab twice to reach every field is a small thing that
/// happens on every field.
fn skip_row_when_tabbing(row: &adw::ActionRow) {
    if contains_entry(row.upcast_ref()) {
        row.set_focusable(false);
        row.set_activatable(false);
    }
}

fn contains_entry(widget: &gtk::Widget) -> bool {
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if widget.is::<gtk::Entry>() || widget.is::<gtk::Text>() {
            return true;
        }
        if contains_entry(&widget) {
            return true;
        }
        child = widget.next_sibling();
    }
    false
}

fn hook_expander(expander: &adw::ExpanderRow) {
    // Connecting twice would scroll twice; the flag is on the widget itself so
    // rebuilding a page can't accumulate handlers.
    unsafe {
        if expander.data::<bool>("pif-expander-hooked").is_some() {
            return;
        }
        expander.set_data("pif-expander-hooked", true);
    }

    expander.connect_expanded_notify(|expander| {
        if !expander.is_expanded() {
            return;
        }
        // After the unfolding animation, when the row has its full height.
        let expander = expander.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(260), move || {
            scroll_into_view(&expander);
        });
    });
}

/// Scroll the nearest scrolling ancestor so the whole widget is visible. A
/// widget taller than the window is aligned to its top instead — showing the end
/// of an explanation and hiding its beginning would be worse than not scrolling.
pub fn scroll_into_view(widget: &impl IsA<gtk::Widget>) {
    let Some(scroller) = widget
        .as_ref()
        .ancestor(gtk::ScrolledWindow::static_type())
        .and_downcast::<gtk::ScrolledWindow>()
    else {
        return;
    };
    let Some(content) = scroller.child() else {
        return;
    };
    let Some(bounds) = widget.as_ref().compute_bounds(&content) else {
        return;
    };

    let adjustment = scroller.vadjustment();
    let page = adjustment.page_size();
    let top = bounds.y() as f64;
    let bottom = top + bounds.height() as f64;
    let visible = adjustment.value()..adjustment.value() + page;

    let target = if bounds.height() as f64 > page || top < visible.start {
        top
    } else if bottom > visible.end {
        // A little breathing room under it, so it doesn't sit against the edge.
        bottom - page + 12.0
    } else {
        return;
    };

    adjustment.set_value(target.clamp(adjustment.lower(), adjustment.upper() - page));
}

/// The build-tools list: a switch per SDK extension, then the languages that
/// need nothing, so "where is C++?" has an answer on the page instead of being
/// a gap someone has to guess about.
pub fn fill_extensions(list: &gtk::ListBox, handle: &Handle) {
    clear(list);
    let chosen = handle.read(|project| project.manifest.sdk_extensions.clone());

    for extension in runtimes::EXTENSIONS {
        let row = adw::SwitchRow::builder()
            .title(t(extension.label))
            .subtitle(t(extension.explanation))
            .subtitle_lines(0)
            .active(chosen.iter().any(|id| id == extension.id))
            .build();
        row.connect_active_notify(clone!(
            #[strong]
            handle,
            move |row| {
                let on = row.is_active();
                handle.write(|project| {
                    let list = &mut project.manifest.sdk_extensions;
                    match (on, list.iter().position(|entry| entry == extension.id)) {
                        (true, None) => list.push(extension.id.to_string()),
                        (false, Some(index)) => {
                            list.remove(index);
                        }
                        _ => {}
                    }
                    // Switching a compiler on is only half of it: the build also
                    // has to be told where the compiler is.
                    runtimes::sync_build_paths(&mut project.manifest);
                });
            }
        ));
        list.append(&row);
    }

    for (language, explanation) in runtimes::ALREADY_INCLUDED {
        let row = adw::ActionRow::builder()
            .title(t(language))
            .subtitle(t(explanation))
            .subtitle_lines(0)
            .build();
        // Not insensitive: greyed-out reads as "unavailable", and the point is
        // the opposite — it is already there.
        row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
        row.add_css_class("note-info");
        list.append(&row);
    }
}

pub fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

/// Build systems offered, with the everyday name and what choosing it means.
/// Shared so the guided steps and the editor cannot end up offering different
/// lists, or describing the same choice differently.
pub const BUILD_SYSTEMS: &[(Option<BuildSystem>, &str, &str)] = &[
    (
        Some(BuildSystem::Simple),
        "Commands I write myself",
        "Nothing is assumed: the commands below are run in order. This is what Rust, \
         Node and hand-written builds use.",
    ),
    (
        Some(BuildSystem::Meson),
        "Meson",
        "Flatpak configures, builds and installs it for you. Projects with a \
         meson.build file use this.",
    ),
    (
        Some(BuildSystem::CMakeNinja),
        "CMake",
        "Flatpak configures, builds and installs it for you, using Ninja — the faster \
         of CMake's two ways of building.",
    ),
    (
        Some(BuildSystem::CMake),
        "CMake with make",
        "As above, but using make instead of Ninja. Only needed if the project says so.",
    ),
    (
        Some(BuildSystem::Autotools),
        "configure and make",
        "The classic setup: a configure script, then make. Projects with a configure \
         or configure.ac file use this.",
    ),
    (
        Some(BuildSystem::QMake),
        "qmake",
        "For Qt projects built with qmake rather than CMake.",
    ),
];

pub fn build_system_index(current: Option<&BuildSystem>) -> u32 {
    BUILD_SYSTEMS
        .iter()
        .position(|(system, _, _)| match (system, current) {
            (Some(a), Some(b)) => a == b,
            // No build system named means "simple"; flatpak-builder's own default.
            (Some(BuildSystem::Simple), None) => true,
            _ => false,
        })
        .unwrap_or(0) as u32
}

pub fn build_system_at(index: u32) -> Option<BuildSystem> {
    BUILD_SYSTEMS
        .get(index as usize)
        .and_then(|(system, _, _)| system.clone())
}

pub fn build_system_explanation(index: u32) -> String {
    BUILD_SYSTEMS
        .get(index as usize)
        .map(|(_, _, explanation)| t(explanation))
        .unwrap_or_default()
}

/// The "What is this?" row a page owes whoever is reading it, for the panes
/// that are built in code rather than in a `.ui` file. The same shape as the
/// ones in `wizard.ui`: a question, and one wrapping line of plain answer.
/// `hook_expanders` is what then makes it scroll itself into view.
pub fn explainer(question: &str, answer: &str) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let expander = adw::ExpanderRow::builder().title(question).build();
    let body = adw::ActionRow::builder()
        .title(answer)
        .title_lines(0)
        // The answers are sentences, and an ampersand or an angle bracket in
        // one would otherwise be read as markup and swallowed.
        .use_markup(false)
        .build();
    body.add_css_class("dim-label");
    expander.add_row(&body);
    group.add(&expander);
    group
}

/// Colour a status row by how serious it is. Only classes are touched, so this
/// is safe to call on every redraw.
pub fn set_note_style(row: &adw::ActionRow, severity: Option<Severity>) {
    row.remove_css_class("note-warning");
    row.remove_css_class("note-info");
    match severity {
        Some(Severity::Error) => row.add_css_class("note-warning"),
        Some(Severity::Warning) => row.add_css_class("note-info"),
        None => {}
    }
}

/// One row of the problems list: what is wrong, what to do, and — when the fix
/// is somewhere else — a way to get there.
pub fn issue_row(issue: &Issue, go_there: Option<Box<dyn Fn()>>) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(&issue.message)
        .subtitle(&issue.fix)
        .title_lines(0)
        .subtitle_lines(0)
        .build();

    let (icon, css) = match issue.severity {
        Severity::Error => ("dialog-warning-symbolic", "note-warning"),
        Severity::Warning => ("dialog-information-symbolic", "note-info"),
    };
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row.add_css_class(css);

    if let Some(go_there) = go_there {
        let button = gtk::Button::builder()
            .label(t("Go there"))
            .valign(gtk::Align::Center)
            .build();
        button.connect_clicked(move |_| go_there());
        row.add_suffix(&button);
    }
    row
}

/// The module as it will appear in the manifest, on its own. Shown live beside
/// the form, so the connection between the questions and the file is never a
/// mystery.
pub fn module_yaml(manifest: &Manifest) -> String {
    let Some(module) = manifest.main_module() else {
        return t("Nothing to build yet.");
    };
    let mut wrapper = Manifest::default();
    wrapper.modules.push(ModuleEntry::Module(module.clone()));
    wrapper
        .to_yaml()
        .unwrap_or_else(|err| err.friendly())
        .trim_start_matches("modules:\n")
        .to_string()
}

// Generic over the view because the manifest pane uses a GtkSourceView, which is
// a GtkTextView with extras.
pub fn text_of(view: &impl IsA<gtk::TextView>) -> String {
    let buffer = view.as_ref().buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

pub fn lines_of(view: &impl IsA<gtk::TextView>) -> Vec<String> {
    text_of(view)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn set_lines(view: &impl IsA<gtk::TextView>, lines: &[String]) {
    view.as_ref().buffer().set_text(&lines.join("\n"));
}

/// `build-options: env:` as one NAME=value per line, and back.
pub fn env_lines(env: &serde_yaml_ng::Mapping) -> Vec<String> {
    env.iter()
        .filter_map(|(key, value)| {
            let key = key.as_str()?;
            let value = value.as_str().map(str::to_string).or_else(|| {
                serde_yaml_ng::to_string(value)
                    .ok()
                    .map(|s| s.trim().to_string())
            })?;
            Some(format!("{key}={value}"))
        })
        .collect()
}

pub fn env_mapping(lines: &[String]) -> serde_yaml_ng::Mapping {
    let mut mapping = serde_yaml_ng::Mapping::new();
    for line in lines {
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                mapping.insert(
                    serde_yaml_ng::Value::from(key),
                    serde_yaml_ng::Value::from(value.trim()),
                );
            }
        }
    }
    mapping
}

/// Write the environment into the module, dropping `build-options` entirely when
/// nothing is left in it rather than leaving an empty stanza behind.
pub fn set_module_env(module: &mut Module, env: serde_yaml_ng::Mapping) {
    match (&mut module.build_options, env.is_empty()) {
        (Some(options), true) => {
            options.env = Default::default();
            if options.is_empty() {
                module.build_options = None;
            }
        }
        (Some(options), false) => options.env = env,
        (None, false) => {
            module.build_options = Some(BuildOptions {
                env,
                ..BuildOptions::default()
            })
        }
        (None, true) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_inside_the_project_are_written_relative() {
        let base = Path::new("/home/me/code/thing");
        assert_eq!(relative_to(Path::new("/home/me/code/thing"), Some(base)), ".");
        assert_eq!(
            relative_to(Path::new("/home/me/code/thing/src"), Some(base)),
            "src"
        );
        assert_eq!(
            relative_to(Path::new("/elsewhere/file.tar"), Some(base)),
            "/elsewhere/file.tar"
        );
        assert_eq!(relative_to(Path::new("/a/b"), None), "/a/b");
    }

    #[test]
    fn a_muted_handle_writes_nothing_and_notifies_nobody() {
        let project = Rc::new(RefCell::new(Project::from_import(
            &packitflat::manifest::parse_str("app-id: no.oyzmo.A\n").unwrap(),
            None,
        )));
        let notified = Rc::new(Cell::new(0));

        let handle = Handle::new(project.clone(), {
            let notified = notified.clone();
            move || notified.set(notified.get() + 1)
        });

        handle.write(|project| project.name = "First".into());
        assert_eq!(project.borrow().name, "First");
        assert_eq!(notified.get(), 1);

        handle.silently(|| handle.write(|project| project.name = "Ignored".into()));
        assert_eq!(project.borrow().name, "First", "muted writes are dropped");
        assert_eq!(notified.get(), 1);

        // …and the mute lifts again afterwards.
        handle.write(|project| project.name = "Second".into());
        assert_eq!(project.borrow().name, "Second");
        assert_eq!(notified.get(), 2);
    }
}
