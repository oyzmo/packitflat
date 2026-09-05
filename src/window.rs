//! The main window: welcome page, and the summary shown after a project is
//! opened.
//!
//! Nothing here decides anything — it asks the library and displays the answer.
//! The `imp` module is kept to the template plumbing; the behaviour is in the
//! `impl PifWindow` block below it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::clone;
use gtk::{gio, glib};

use packitflat::detect::Detection;
use packitflat::i18n::t;
use packitflat::manifest::{self, ImportReport, NoteLevel};
use packitflat::project::{self, Project};
use packitflat::VERSION;

use packitflat::{build, settings};

use crate::build_page::PifBuild;
use crate::editor::PifEditor;
use crate::wizard::PifWizard;

mod imp {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/no/oyzmo/PackItFlat/ui/window.ui")]
    pub struct PifWindow {
        #[template_child]
        pub nav: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub new_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub import_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub template_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub clone_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub setup_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub setup_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub recent_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub recent_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub version_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub project_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub detected_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub detected_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub row_app_id: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub row_runtime: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub row_command: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub row_source: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub notes_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub notes_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub yaml_view: TemplateChild<gtk::TextView>,

        #[template_child]
        pub wizard_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub editor_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub build_button: TemplateChild<gtk::Button>,

        /// The single copy of the project, shared with the wizard rather than
        /// copied into it: two views, one value, nothing to keep in step.
        pub project: RefCell<Option<Rc<RefCell<Project>>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PifWindow {
        const NAME: &'static str = "PifWindow";
        type Type = super::PifWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PifWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PifWindow {}
    impl WindowImpl for PifWindow {}
    impl ApplicationWindowImpl for PifWindow {}
    impl AdwApplicationWindowImpl for PifWindow {}
}

glib::wrapper! {
    pub struct PifWindow(ObjectSubclass<imp::PifWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl PifWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    fn setup(&self) {
        let imp = self.imp();
        imp.version_label.set_label(&format!("v{VERSION}"));

        imp.new_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.choose_folder()
        ));
        imp.import_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.choose_manifest()
        ));
        imp.wizard_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.open_wizard()
        ));
        imp.editor_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.open_editor()
        ));
        imp.build_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.open_build()
        ));
        imp.template_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.choose_template()
        ));
        imp.clone_button.connect_clicked(clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.clone_from_git()
        ));

        // Coming back from either mode means the project has changed underneath
        // the summary page — and may mean the user asked for the other mode.
        imp.nav.connect_popped(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, page| {
                window.refresh_project_views();
                window.handle_mode_switch(page);
            }
        ));

        let preferences = gio::SimpleAction::new("preferences", None);
        preferences.connect_activate(clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.show_preferences()
        ));
        self.add_action(&preferences);

        self.refresh_recents();
        self.check_the_computer();
        crate::forms::hook_expanders(self);
    }

    /// The first-run check: is there anything on this computer that would stop a
    /// build later? Asked once, in the background, and only shown when the
    /// answer is "yes" — a checklist of ticks nobody needs to see.
    fn check_the_computer(&self) {
        glib::spawn_future_local(clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let sandboxed = Path::new("/.flatpak-info").exists();
                let probe = build::Probe {
                    flatpak_present: crate::proc::succeeds(&host_args(
                        &["flatpak", "--version"],
                        sandboxed,
                    ))
                    .await,
                    builder_present: crate::proc::succeeds(&host_args(
                        &["flatpak-builder", "--version"],
                        sandboxed,
                    ))
                    .await,
                    flathub: crate::proc::output(&host_args(
                        &["flatpak", "remotes", "--columns=name,options"],
                        sandboxed,
                    ))
                    .await
                    .map(|(text, _)| build::remote_scope(&text, "flathub"))
                    .unwrap_or_default(),
                    ..build::Probe::default()
                };

                let checks = build::environment_checks(&probe);
                let unready: Vec<&build::Check> = checks
                    .iter()
                    .filter(|check| check.state != build::State::Ready)
                    .collect();

                let imp = window.imp();
                clear_list(&imp.setup_list);
                for check in &unready {
                    imp.setup_list.append(&window.setup_row(check));
                }
                imp.setup_group.set_visible(!unready.is_empty());
            }
        ));
    }

    fn setup_row(&self, check: &build::Check) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(&check.title)
            .subtitle(&check.detail)
            .title_lines(0)
            .subtitle_lines(0)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(match check.state {
            build::State::Blocked => "dialog-warning-symbolic",
            _ => "dialog-information-symbolic",
        }));
        row.add_css_class(match check.state {
            build::State::Blocked => "note-warning",
            _ => "note-info",
        });

        if let Some(fix) = &check.fix {
            let (label, command, runnable) = match fix {
                build::Fix::Run { label, command } => (label.clone(), Some(command.clone()), true),
                build::Fix::Copy { label, command } => (label.clone(), Some(command.clone()), false),
                build::Fix::Elsewhere { label, .. } => (label.clone(), None, false),
            };
            let Some(command) = command else {
                return row;
            };

            let button = gtk::Button::builder()
                .label(label)
                .valign(gtk::Align::Center)
                .build();
            button.connect_clicked(clone!(
                #[weak(rename_to = window)]
                self,
                move |button| {
                    if runnable {
                        let argv = command.argv.clone();
                        button.set_sensitive(false);
                        glib::spawn_future_local(clone!(
                            #[weak]
                            window,
                            #[weak]
                            button,
                            async move {
                                let _ = crate::proc::output(&argv).await;
                                button.set_sensitive(true);
                                window.check_the_computer();
                            }
                        ));
                    } else {
                        window.clipboard().set_text(&command.as_typed());
                        button.set_label(&t("Copied"));
                    }
                }
            ));
            row.add_suffix(&button);
        }

        row
    }

    // -- Opening a project ---------------------------------------------------

    /// "Start with a folder": pick a folder, say what was found in it, and turn
    /// it into a project with defaults already filled in.
    fn choose_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title(t("Choose the folder your program's code is in"))
            .modal(true)
            .build();

        glib::spawn_future_local(clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let file = match dialog.select_folder_future(Some(&window)).await {
                    Ok(file) => file,
                    Err(err) => return window.report_dialog_error(&err),
                };
                let Some(path) = file.path() else {
                    return window.show_error(
                        &t("That folder can't be used"),
                        &t("It isn't a folder on this computer's file system."),
                    );
                };
                let (project, detection) = Project::from_folder(&path);
                window.open_project(project, Some(detection), None);
                // A brand new project goes straight to work; reopening one from
                // the recent list stays on the summary, where it started.
                window.open_preferred_mode();
            }
        ));
    }

    /// "Open a manifest I already have": import YAML or JSON, then report what
    /// could and couldn't be understood.
    fn choose_manifest(&self) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&t("Flatpak manifests")));
        for pattern in ["*.yml", "*.yaml", "*.json"] {
            filter.add_pattern(pattern);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);

        let dialog = gtk::FileDialog::builder()
            .title(t("Choose a manifest file"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();

        glib::spawn_future_local(clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let file = match dialog.open_future(Some(&window)).await {
                    Ok(file) => file,
                    Err(err) => return window.report_dialog_error(&err),
                };
                let Some(path) = file.path() else {
                    return window.show_error(
                        &t("That file can't be opened"),
                        &t("It isn't a file on this computer's file system."),
                    );
                };
                if window.import_manifest(&path) {
                    window.open_preferred_mode();
                }
            }
        ));
    }

    /// "Start from a Git address": fetch the code once so it can be looked at,
    /// then carry on exactly as if the folder had been picked by hand — except
    /// that the manifest names the repository rather than this computer.
    fn clone_from_git(&self) {
        let dialog = crate::clone_page::PifClone::new(clone!(
            #[weak(rename_to = window)]
            self,
            move |project| {
                window.open_project(project, None, None);
                window.open_preferred_mode();
            }
        ));
        dialog.present(Some(self));
    }

    /// "Start from a template": for when there is no code yet. Each one is a
    /// project set up the way that kind of app usually is, so the answer to
    /// "what do I even put here" is already filled in.
    fn choose_template(&self) {
        let dialog = adw::Dialog::builder()
            .title(t("Start from a template"))
            .content_width(560)
            .content_height(520)
            .build();

        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .description(t(
                "Each of these sets the runtime, the build system, the tools that kind \
                 of app needs and the permissions it usually asks for. You can change \
                 any of it afterwards.",
            ))
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .build();
        list.add_css_class("boxed-list");

        for template in packitflat::templates::TEMPLATES {
            let row = adw::ActionRow::builder()
                .title(t(template.name))
                .subtitle(format!("{}\n{}", t(template.description), t(template.detail)))
                .subtitle_lines(0)
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            row.connect_activated(clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                dialog,
                move |_| {
                    dialog.close();
                    window.start_from_template(template);
                }
            ));
            list.append(&row);
        }

        group.add(&list);
        page.add(&group);

        let view = adw::ToolbarView::builder().content(&page).build();
        view.add_top_bar(&adw::HeaderBar::new());
        dialog.set_child(Some(&view));
        dialog.present(Some(self));
    }

    /// A template still needs somewhere to live: the manifest and everything
    /// beside it are written into a folder, so that is the one question left.
    fn start_from_template(&self, template: &'static packitflat::templates::Template) {
        let dialog = gtk::FileDialog::builder()
            .title(t("Where should the project live?"))
            .modal(true)
            .accept_label(t("Use this folder"))
            .build();

        glib::spawn_future_local(clone!(
            #[weak(rename_to = window)]
            self,
            async move {
                let Ok(file) = dialog.select_folder_future(Some(&window)).await else {
                    return;
                };
                let Some(folder) = file.path() else { return };

                let name = folder
                    .file_name()
                    .map(|name| title_case(&name.to_string_lossy()))
                    .unwrap_or_else(|| t("My App"));

                let project = packitflat::templates::apply(template, &name, Some(&folder));
                window.open_project(project, None, None);
                window.open_preferred_mode();
            }
        ));
    }

    /// Open whatever was handed to the app: a folder is a project to package,
    /// a file is a manifest to import.
    pub fn open_path(&self, path: &Path) {
        if path.is_dir() {
            let (project, detection) = Project::from_folder(path);
            self.open_project(project, Some(detection), None);
        } else {
            self.import_manifest(path);
        }
    }

    /// Returns whether the manifest was actually opened, so the caller knows
    /// whether there is a project to carry on into a mode with.
    fn import_manifest(&self, path: &Path) -> bool {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) => {
                self.show_error(
                    &t("That file couldn't be read"),
                    &format!(
                        "{}\n\n{err}",
                        t("Check that it still exists and that you have permission to read it.")
                    ),
                );
                return false;
            }
        };

        match manifest::parse_str(&text) {
            Ok(import) => {
                let project = Project::from_import(&import, Some(path));
                self.open_project(project, None, Some(import.report));
                return true;
            }
            Err(err) => self.show_error(
                &t("This doesn't look like a manifest this app can read"),
                &format!(
                    "{}\n\n{}",
                    err.friendly(),
                    t("A manifest is a YAML or JSON file listing settings such as \
                       app-id, runtime and modules.")
                ),
            ),
        }
        false
    }

    fn open_recent(&self, path: &Path) {
        match project::load(path) {
            Ok(project) => self.open_project(project, None, None),
            Err(_) => self.show_error(
                &t("That project couldn't be reopened"),
                &t("The saved copy is damaged or was written by a newer version of \
                    this app. Your manifest file, if you already generated one, is \
                    untouched — you can open that instead."),
            ),
        }
    }

    /// The one path into the project view. Fills the summary, saves the project
    /// so an interrupted session survives, and shows the page.
    fn open_project(
        &self,
        project: Project,
        detection: Option<Detection>,
        report: Option<ImportReport>,
    ) {
        let imp = self.imp();
        let mut project = project;

        // An icon already sitting where this app puts them belongs to this
        // project, whatever the saved copy remembers — a manifest opened on its
        // own remembers nothing, and the icon would go missing from the plan
        // and from the build.
        packitflat::icons::adopt_existing(&mut project);

        // Before anything is shown or saved. A hand-written build installs only
        // what its commands say, and this used to be put right in the two modes'
        // `collect` — which runs when a widget changes, so opening a manifest
        // that was missing the lines and pressing "Write the manifest" without
        // touching a build field wrote it back exactly as broken as it arrived.
        // The symptom is a Flatpak holding the program and nothing else: no menu
        // entry, and a generic icon in every Flatpak manager. (A manifest this
        // app itself had written did exactly that.)
        packitflat::generate::sync_install_commands(&mut project);

        imp.project_page.set_title(&project.name);
        self.fill_detection(detection.as_ref(), &project);
        self.fill_summary(&project);
        self.fill_notes(report.as_ref());
        self.fill_yaml(&project);

        if let Err(err) = project::save(&project) {
            self.show_error(
                &t("This project couldn't be saved"),
                &format!(
                    "{}\n\n{err:#}",
                    t("You can carry on working, but the app won't be able to \
                       reopen this project for you if it closes.")
                ),
            );
        }

        imp.project.replace(Some(Rc::new(RefCell::new(project))));
        self.refresh_recents();
        imp.nav.push_by_tag("project");
    }

    /// Repaint the summary from the shared project — used when the wizard hands
    /// control back, since it has been editing the very same value.
    fn refresh_project_views(&self) {
        let Some(project) = self.imp().project.borrow().clone() else {
            return;
        };
        let project = project.borrow();
        self.imp().project_page.set_title(&project.name);
        self.fill_detection(None, &project);
        self.fill_summary(&project);
        self.fill_yaml(&project);
    }

    fn open_wizard(&self) {
        let Some(project) = self.imp().project.borrow().clone() else {
            return;
        };
        self.imp().nav.push(&PifWizard::new(project));
    }

    fn open_editor(&self) {
        let Some(project) = self.imp().project.borrow().clone() else {
            return;
        };
        self.imp().nav.push(&PifEditor::new(project));
    }

    fn open_build(&self) {
        let Some(project) = self.imp().project.borrow().clone() else {
            return;
        };
        self.imp().nav.push(&PifBuild::new(project));
    }

    /// Open a project in whichever way the user works. The question is asked once
    /// and remembered; both answers reach the same project, so a wrong guess
    /// costs one click on the mode switcher rather than any work.
    fn open_preferred_mode(&self) {
        match settings::default_mode() {
            settings::Mode::Guided => self.open_wizard(),
            settings::Mode::Editor => self.open_editor(),
            settings::Mode::Ask => self.ask_for_mode(),
        }
    }

    fn ask_for_mode(&self) {
        let dialog = adw::AlertDialog::builder()
            .heading(t("How would you like to work?"))
            .body(t(
                "Guided steps ask one question at a time and explain each answer. The \
                 editor shows everything at once, including the manifest as text. Both \
                 change the same project, and you can switch at any time.",
            ))
            .build();
        dialog.add_response("guided", &t("Guided steps"));
        dialog.add_response("editor", &t("Editor"));
        dialog.set_response_appearance("guided", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("guided"));

        // The checkbox has to be built here: an alert dialog's extra child is not
        // something the .ui file can hold on to.
        let remember = gtk::CheckButton::builder()
            .label(t("Remember my choice"))
            .margin_top(6)
            .build();
        dialog.set_extra_child(Some(&remember));

        dialog.connect_response(
            None,
            clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                remember,
                move |_, response| {
                    let mode = match response {
                        "editor" => settings::Mode::Editor,
                        _ => settings::Mode::Guided,
                    };
                    if remember.is_active() {
                        settings::set_default_mode(mode);
                    }
                    match mode {
                        settings::Mode::Editor => window.open_editor(),
                        _ => window.open_wizard(),
                    }
                }
            ),
        );
        dialog.present(Some(self));
    }

    /// Leaving one mode through its switcher means entering the other over the
    /// same project — the page is gone by the time this runs, which is why the
    /// departing page is asked what it wanted.
    fn handle_mode_switch(&self, page: &adw::NavigationPage) {
        if let Some(wizard) = page.downcast_ref::<PifWizard>() {
            if wizard.wanted_editor() {
                self.open_editor();
            }
        } else if let Some(editor) = page.downcast_ref::<PifEditor>() {
            if editor.wanted_guided() {
                self.open_wizard();
            }
        }
    }

    fn show_preferences(&self) {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title(&t("Preferences"));

        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .title(t("New projects"))
            .description(t(
                "Which way of working a project opens in. You can always switch from \
                 the header bar afterwards.",
            ))
            .build();

        let modes = [
            settings::Mode::Ask,
            settings::Mode::Guided,
            settings::Mode::Editor,
        ];
        let labels = gtk::StringList::new(&[]);
        for mode in modes {
            labels.append(&t(mode.label()));
        }

        let row = adw::ComboRow::builder()
            .title(t("Open new projects in"))
            .model(&labels)
            .selected(
                modes
                    .iter()
                    .position(|mode| *mode == settings::default_mode())
                    .unwrap_or(0) as u32,
            )
            .build();
        row.connect_selected_notify(move |row| {
            if let Some(mode) = modes.get(row.selected() as usize) {
                settings::set_default_mode(*mode);
            }
        });
        group.add(&row);

        if !settings::schema_installed() {
            let note = adw::ActionRow::builder()
                .title(t("Saved in this app's own settings file"))
                .subtitle(format!(
                    "{}\n{}",
                    t("The system settings schema isn't installed, so preferences are \
                       kept here instead:"),
                    settings::config_path().display()
                ))
                .subtitle_lines(0)
                .build();
            note.add_prefix(&gtk::Image::from_icon_name("dialog-information-symbolic"));
            note.add_css_class("note-info");
            group.add(&note);
        }

        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }

    /// The project every hook below needs, or a sentence saying how to give it
    /// one. None of them can open a project themselves, so a harness started
    /// without a path used to fail with "no project is open" — true, but not
    /// the same thing as the check having found something, and one of them
    /// (`PACKITFLAT_DEV_EDITOR`) simply hung instead.
    #[cfg(debug_assertions)]
    fn dev_project(&self, check: &str) -> Rc<RefCell<Project>> {
        match self.imp().project.borrow().clone() {
            Some(project) => project,
            None => {
                eprintln!(
                    "{check}: FAIL no project is open — start the app with one, \
                     e.g. packitflat dev/probe/hello/no.oyzmo.Hello.yml"
                );
                std::process::exit(1);
            }
        }
    }

    /// Write the files and show what happens afterwards, for the harness.
    #[cfg(debug_assertions)]
    pub fn dev_write_files(&self) {
        let project = self.dev_project("write");
        let handle = crate::forms::Handle::new(project, || {});
        crate::forms::write_manifest(&handle, self, true);
    }

    /// Type into the guided steps, switch to the editor, and check that what was
    /// typed is still there. Switching modes must not lose a field — the whole
    /// design rests on both modes editing one project.
    #[cfg(debug_assertions)]
    pub fn dev_switch_check(&self) {
        let project = self.dev_project("switch-check");

        let wizard = crate::wizard::PifWizard::new(project.clone());
        self.imp().nav.push(&wizard);
        wizard.dev_fill_basics();

        let window = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("switch-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };

            // In the project itself, as the guided steps left it.
            {
                let project = project.borrow();
                check("the name reached the project", project.name == "Typed Name");
                check("the developer did", project.developer == "Typed Developer");
                check("the website did", project.homepage == "http://typed.example");
                check(
                    "and the app ID did",
                    project.manifest.app_id == "no.oyzmo.Typed",
                );
            }

            // And in the editor, which is where they looked empty.
            let editor = crate::editor::PifEditor::new(project.clone());
            window.imp().nav.push(&editor);

            // Rebuilding the pane — which a YAML edit does on every keystroke
            // that parses — must replace the fields, not stack another set of
            // them on top of the first.
            let before = editor.dev_manifest_fields().len();
            editor.dev_reload_forms();
            editor.dev_reload_forms();
            check(
                "rebuilding the pane doesn't duplicate the fields",
                editor.dev_manifest_fields().len() == before,
            );

            let shown = editor.dev_manifest_fields();
            eprintln!("switch-check: editor shows {shown:?}");
            check(
                "the editor shows the developer",
                shown.iter().any(|value| value == "Typed Developer"),
            );
            check(
                "the editor shows the website",
                shown.iter().any(|value| value == "http://typed.example"),
            );
            check(
                "the editor shows the app ID",
                shown.iter().any(|value| value == "no.oyzmo.Typed"),
            );

            eprintln!(
                "switch-check: {}",
                if ok { "all checks passed" } else { "FAILURES ABOVE" }
            );
            window.close();
            if !ok {
                std::process::exit(1);
            }
        });
    }

    /// Scroll the visible page to the bottom, so the harness can photograph what
    /// sits below the fold.
    #[cfg(debug_assertions)]
    pub fn dev_scroll_to_end(&self) {
        let page = self
            .imp()
            .nav
            .visible_page()
            .map(|page| page.upcast::<gtk::Widget>())
            .unwrap_or_else(|| self.clone().upcast());

        if let Some(scroller) = find_scroller(&page) {
            // Twice: the page grows when the background checks finish, and a
            // scroll from before that lands short of the end.
            for delay in [300, 1000] {
                let adjustment = scroller.vadjustment();
                glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
                    adjustment.set_value(adjustment.upper() - adjustment.page_size());
                });
            }
        }
    }

    /// Open the first "What is this?" row and check that the window actually
    /// scrolled to show it. The scrolling itself is a couple of lines of
    /// arithmetic; whether it *happens* is the part worth checking.
    #[cfg(debug_assertions)]
    pub fn dev_expander_check(&self) {
        // After the carousel has finished moving. `go_to_step` scrolls with an
        // animation that doesn't even start until the carousel has been
        // allocated, so asking straight away is asking about step 1 whichever
        // step was opened — which is why this used to report the same scroll
        // distance for all eight of them. Waiting for the position to stop
        // changing rather than for a fixed delay, because the delay that is
        // long enough on this computer is a guess anywhere else.
        let window = self.clone();
        let last = Rc::new(RefCell::new(f64::NAN));
        let ticks = Rc::new(RefCell::new(0u32));
        glib::timeout_add_local(std::time::Duration::from_millis(150), move || {
            *ticks.borrow_mut() += 1;
            let waited = *ticks.borrow();
            let settled = match dev_carousel(&window) {
                None => true,
                Some(carousel) => {
                    let now = carousel.position();
                    let before = last.replace(now);
                    before == now
                }
            };
            // Two ticks minimum: a page that has only just been pushed has not
            // been allocated yet, and an unallocated carousel reports 0.
            if (settled && waited >= 2) || waited > 20 {
                window.dev_expander_check_now();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    #[cfg(debug_assertions)]
    fn dev_expander_check_now(&self) {
        // Whatever page is on top: with the wizard open, the welcome page is
        // still in the stack behind it and would be found first.
        let page = self
            .imp()
            .nav
            .visible_page()
            .map(|page| page.upcast::<gtk::Widget>())
            .unwrap_or_else(|| self.clone().upcast());

        // Every one on the page, not just the first: the explanation a page
        // owes the reader is usually its last row, so a check that opened only
        // the first was reporting on the source row or the category list.
        let rows = visible_expanders(&page);
        if rows.is_empty() {
            // Nothing to check is not the same as something being wrong.
            // Closing, like the end of the check does, so the harness gets its
            // output back instead of a killed process.
            eprintln!("expand-check: skipped — this page has no explanation to open");
            if std::env::var_os(crate::devshot::ENV_VAR).is_none() {
                self.close();
            }
            return;
        }
        eprintln!("expand-check:      {} to open on this page", rows.len());
        self.dev_open_one(rows, 0, true);
    }

    /// One row, then the next: opening them all at once would measure each of
    /// them against a page the others had already moved.
    #[cfg(debug_assertions)]
    fn dev_open_one(&self, rows: Vec<adw::ExpanderRow>, index: usize, ok_so_far: bool) {
        let Some(expander) = rows.get(index).cloned() else {
            eprintln!(
                "expand-check: {}",
                if ok_so_far { "all checks passed" } else { "FAILURES ABOVE" }
            );
            self.close();
            if !ok_so_far {
                std::process::exit(1);
            }
            return;
        };
        eprintln!("expand-check:      opening “{}”", expander.title());
        // A row that isn't inside anything that scrolls is not a failure: the
        // raw YAML pane keeps its explanation above the editor, where opening
        // it takes the room from the text view rather than moving the page.
        // The requirement is the same either way — all of it on screen — so
        // there the window itself is what it has to fit inside.
        let scroller = expander
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();

        let before = scroller
            .as_ref()
            .map(|scroller| scroller.vadjustment().value())
            .unwrap_or_default();
        expander.set_expanded(true);

        let window = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("expand-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };

            // Scrolling is the means, not the requirement. An expander near the
            // top of a page opens with all of it already on screen and nothing
            // to scroll — demanding a scroll there fails a page that is doing
            // exactly the right thing, which is what happened the first time
            // this was pointed at the editor.
            let (container, visible) = match &scroller {
                Some(scroller) => {
                    let adjustment = scroller.vadjustment();
                    eprintln!(
                        "expand-check:      scrolled by {:.0}px",
                        adjustment.value() - before
                    );
                    (
                        scroller.child().map(|content| content.upcast()),
                        adjustment.value()..adjustment.value() + adjustment.page_size(),
                    )
                }
                None => {
                    eprintln!("expand-check:      nothing to scroll — it sits above the page");
                    let window: gtk::Widget = window.clone().upcast();
                    let height = window.height() as f64;
                    (Some(window), 0.0..height)
                }
            };

            let bottom = container
                .and_then(|content| expander.compute_bounds(&content))
                .map(|bounds| (bounds.y() + bounds.height()) as f64);
            check(
                "the whole explanation is on screen",
                bottom.is_some_and(|bottom| bottom <= visible.end + 1.0),
            );

            // With a screenshot pending, stop at the first one and leave it
            // open: an opened explanation is a thing worth photographing, and
            // carrying on would close it again before the camera got there.
            if std::env::var_os(crate::devshot::ENV_VAR).is_some() {
                eprintln!(
                    "expand-check: {}",
                    if ok { "all checks passed" } else { "FAILURES ABOVE" }
                );
                return;
            }

            // Closed again, so the next row is measured against the page as
            // someone would actually meet it.
            expander.set_expanded(false);
            window.dev_open_one(rows, index + 1, ok_so_far && ok);
        });
    }

    /// Fetch a repository and report what came of it. The only way to check this
    /// path is to actually clone something, so the harness does.
    #[cfg(debug_assertions)]
    pub fn dev_clone(&self, url: &str, folder: &Path) {
        let folder_for_check = folder.to_path_buf();
        let dialog = crate::clone_page::PifClone::new(clone!(
            #[weak(rename_to = window)]
            self,
            move |project: Project| {
                let mut ok = true;
                let mut check = |what: &str, passed: bool| {
                    eprintln!("clone-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                    ok &= passed;
                };

                check(
                    "the code was fetched into the folder",
                    project.source_dir.as_deref() == Some(folder_for_check.as_path()),
                );
                check(
                    "the app looked at what it fetched",
                    project.detected.is_some(),
                );

                let source = project
                    .manifest
                    .main_module()
                    .and_then(|module| module.sources.first())
                    .and_then(|entry| entry.as_source().cloned());
                match source {
                    Some(source) => {
                        check(
                            "the manifest points at the repository, not this computer",
                            source.kind == packitflat::manifest::SourceKind::Git
                                && source.url.is_some(),
                        );
                        check(
                            "and is pinned to the exact revision",
                            source
                                .commit
                                .as_deref()
                                .is_some_and(|commit| commit.len() >= 7),
                        );
                    }
                    None => check("the manifest has a source", false),
                }

                eprintln!(
                    "clone-check: {}",
                    if ok { "all checks passed" } else { "FAILURES ABOVE" }
                );
                window.close();
                if !ok {
                    std::process::exit(1);
                }
            }
        ));

        dialog.present(Some(self));
        dialog.dev_start(url, folder);

        // A failure never reaches the callback, so it is reported from here.
        let dialog_for_watch = dialog.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(20), move || {
            if let Some(problem) = dialog_for_watch.dev_failure() {
                eprintln!("clone-check: FAIL {problem}");
                std::process::exit(1);
            }
        });
    }

    /// Open the build page, optionally with a stand-in build running, for the
    /// harness — this machine has no flatpak-builder to drive a real one.
    #[cfg(debug_assertions)]
    pub fn dev_open_build(&self, mode: &str) {
        let Some(project) = self.imp().project.borrow().clone() else {
            return;
        };
        let page = PifBuild::new(project);
        self.imp().nav.push(&page);

        match mode {
            "run" => page.dev_run(
                "echo 'Downloading sources'; echo 'Building module cleaner'; \
                 echo '   Compiling serde v1.0.0'; sleep 20",
            ),
            "ok" => page.dev_run("echo 'Exporting no.oyzmo.Risky'; true"),
            "fail" => page.dev_run(
                "echo 'Building module cleaner'; \
                 echo 'error: Failed to init: Unable to find sdk org.gnome.Sdk version 50'; \
                 exit 1",
            ),
            "check" => page.dev_build_check(),
            "terminal" => page.dev_terminal_check(),
            _ => {}
        }
    }

    /// Type into the guided steps, press nothing, and check that the saved copy
    /// on disk caught up on its own.
    ///
    /// The saved copy exists to survive an interruption, and it used to be
    /// written only when the page changed or the mode was switched — so whatever
    /// was typed on the page you were on was exactly what a crash took. Nothing
    /// here navigates: that is the point.
    #[cfg(debug_assertions)]
    pub fn dev_autosave_check(&self) {
        let project = self.dev_project("autosave-check");

        let saved = packitflat::project::projects_dir()
            .join(format!("{}.yml", project.borrow().slug()));
        let _ = std::fs::remove_file(&saved);

        let wizard = crate::wizard::PifWizard::new(project);
        self.imp().nav.push(&wizard);

        let mut ok = true;
        let mut check = |what: &str, passed: bool| {
            eprintln!("autosave-check: {} {what}", if passed { "ok  " } else { "FAIL" });
            ok &= passed;
        };
        check("nothing is saved before anything is typed", !saved.exists());
        wizard.dev_fill_basics();
        check("typing alone doesn't write the file at once", !saved.exists());
        if !ok {
            std::process::exit(1);
        }

        // Longer than the wait itself, and the check is what the file says
        // rather than that some file appeared.
        glib::timeout_add_local_once(std::time::Duration::from_millis(2500), move || {
            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("autosave-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };

            check("the file is written without pressing anything", saved.exists());
            let text = std::fs::read_to_string(&saved).unwrap_or_default();
            check("it holds what was typed", text.contains("Typed Developer"));
            check("including the app ID", text.contains("no.oyzmo.Typed"));

            eprintln!(
                "autosave-check: {}",
                if ok { "all checks passed" } else { "some checks failed" }
            );
            std::process::exit(if ok { 0 } else { 1 });
        });
    }

    /// Report the narrowest the page on top could be, and fail if a phone
    /// couldn't show it.
    ///
    /// A minimum width is invisible: a pane squeezed to 341px and a pane that
    /// insists on 341px look identical once laid out, and the only symptom of
    /// the second is content quietly drawn past the edge of the window. This
    /// asks the widget instead. Pair it with `PACKITFLAT_DEV_EDITOR=<pane>`,
    /// which is the only way to reach one pane at a time now that the panes
    /// stack measures just the visible one.
    #[cfg(debug_assertions)]
    pub fn dev_narrow_check(&self, target: i32) {
        // The window has to actually *be* narrow first. A breakpoint applies on
        // allocation, so measuring at the default size reports the uncollapsed
        // minimum — 700px for the editor — which is the number the breakpoint
        // exists to avoid rather than the one being asked about.
        self.set_default_size(target, 780);

        let window = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
            let page = window
                .imp()
                .nav
                .visible_page()
                .map(|page| page.upcast::<gtk::Widget>())
                .unwrap_or_else(|| window.clone().upcast());

            // The page's own minimum is the breakpoint bin's declared one, which
            // is the answer we are trying to check rather than the one we want.
            // What decides whether the window can be this narrow is the child
            // inside it.
            let inside = crate::forms::descendant::<adw::NavigationSplitView>(&page)
                .map(|split| split.upcast::<gtk::Widget>())
                .unwrap_or_else(|| page.clone());
            let (min, _, _, _) = inside.measure(gtk::Orientation::Horizontal, -1);

            let fits = min <= target;
            eprintln!(
                "narrow-check: {} {} needs {min}px, and a phone offers {target}px",
                if fits { "ok  " } else { "FAIL" },
                inside.type_().name(),
            );
            window.close();
            std::process::exit(if fits { 0 } else { 1 });
        });
    }

    /// Show one of the dialogs, for the screenshot harness: they are otherwise
    /// only reachable through a file chooser or a menu.
    #[cfg(debug_assertions)]
    pub fn dev_show_dialog(&self, which: &str) {
        match which {
            "mode" => self.ask_for_mode(),
            "preferences" => self.show_preferences(),
            "templates" => self.choose_template(),
            "clone" => self.clone_from_git(),
            "licence" => {
                let license = self
                    .imp()
                    .project
                    .borrow()
                    .as_ref()
                    .map(|project| project.borrow().license.clone())
                    .unwrap_or_default();
                crate::forms::choose_licence(self, &license, |_| {});
            }
            "forget" => {
                if let Some(recent) = project::recents().first() {
                    self.forget_recent(&recent.path);
                }
            }
            _ => {}
        }
    }

    /// Jump straight into the editor at a given pane, for the screenshot harness.
    #[cfg(debug_assertions)]
    pub fn dev_open_editor(&self, pane: &str) -> PifEditor {
        let project = self.dev_project("editor");
        let editor = PifEditor::new(project);
        self.imp().nav.push(&editor);
        editor.dev_show_pane(pane);
        editor
    }

    /// Jump straight into the wizard at a given step. Only used by the
    /// screenshot harness, which has no way to click its way there.
    #[cfg(debug_assertions)]
    pub fn dev_open_wizard(&self, step: u32) {
        let project = self.dev_project("wizard");
        let wizard = PifWizard::new(project);
        self.imp().nav.push(&wizard);
        wizard.go_to_step(step);
    }

    /// Drive the licence picker's search the way a person would.
    #[cfg(debug_assertions)]
    pub fn dev_licence_check(&self, query: &str) {
        let project = self.dev_project("licence-check");
        let wizard = PifWizard::new(project);
        self.imp().nav.push(&wizard);
        wizard.dev_licence_check(query);
    }

    // -- Filling the project view -------------------------------------------

    fn fill_detection(&self, detection: Option<&Detection>, project: &Project) {
        let imp = self.imp();
        match detection {
            Some(detection) => {
                imp.detected_row.set_title(detection.kind.title());
                imp.detected_row.set_subtitle(&detection.explanation());
                imp.detected_group.set_visible(true);
            }
            None => {
                // A reopened project remembers what was found, but not the
                // sentence — repeating the conclusion is honest, inventing the
                // reasoning again is not.
                match &project.detected {
                    Some(kind) => {
                        imp.detected_row.set_title(kind);
                        imp.detected_row.set_subtitle(&t(
                            "This is what was found in the source folder when the \
                             project was set up.",
                        ));
                        imp.detected_group.set_visible(true);
                    }
                    None => imp.detected_group.set_visible(false),
                }
            }
        }
    }

    fn fill_summary(&self, project: &Project) {
        let imp = self.imp();
        let m = &project.manifest;

        set_value(
            &imp.row_app_id,
            &m.app_id,
            &t("Not chosen yet — you'll be asked for this first."),
        );
        let runtime = if m.runtime.is_empty() {
            String::new()
        } else if m.runtime_version.is_empty() {
            m.runtime.clone()
        } else {
            format!("{} {}", friendly_runtime(&m.runtime), m.runtime_version)
        };
        set_value(
            &imp.row_runtime,
            &runtime,
            &t("The set of libraries the app runs on. GNOME is the usual choice."),
        );
        set_value(
            &imp.row_command,
            &m.command,
            &t("The program that starts when someone opens the app."),
        );

        // For a project started from a folder, the folder is the honest answer.
        // For an imported manifest the folder is just where the file happened to
        // sit, so the manifest's own first source is what to show.
        let source = match (project.imported_from.is_some(), &project.source_dir) {
            (false, Some(dir)) => dir.display().to_string(),
            _ => m
                .main_module()
                .and_then(|module| module.sources.first())
                .map(describe_source)
                .unwrap_or_default(),
        };
        set_value(
            &imp.row_source,
            &source,
            &t("The folder or repository the app is built from."),
        );
    }

    fn fill_notes(&self, report: Option<&ImportReport>) {
        let imp = self.imp();
        clear_list(&imp.notes_list);

        let Some(report) = report.filter(|r| !r.is_empty()) else {
            imp.notes_group.set_visible(false);
            return;
        };

        for note in &report.notes {
            let row = adw::ActionRow::builder()
                .title(&note.where_)
                .subtitle(&note.message)
                .subtitle_lines(0)
                .build();
            let (icon, css) = match note.level {
                NoteLevel::Warning => ("dialog-warning-symbolic", "note-warning"),
                NoteLevel::Info => ("dialog-information-symbolic", "note-info"),
            };
            row.add_css_class(css);
            row.add_prefix(&gtk::Image::from_icon_name(icon));
            imp.notes_list.append(&row);
        }
        imp.notes_group.set_visible(true);
    }

    fn fill_yaml(&self, project: &Project) {
        let text = project
            .manifest
            .to_yaml()
            .unwrap_or_else(|err| err.friendly());
        self.imp().yaml_view.buffer().set_text(&text);
    }

    fn refresh_recents(&self) {
        let imp = self.imp();
        clear_list(&imp.recent_list);

        let recents = project::recents();
        for recent in recents.iter().take(5) {
            let row = adw::ActionRow::builder()
                .title(&recent.name)
                .subtitle(&recent.subtitle)
                .activatable(true)
                .build();
            let path: PathBuf = recent.path.clone();

            let forget = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .valign(gtk::Align::Center)
                .tooltip_text(t("Take this off the list. Your project isn't touched."))
                .build();
            forget.add_css_class("flat");
            forget.connect_clicked(clone!(
                #[weak(rename_to = window)]
                self,
                #[strong]
                path,
                move |_| window.forget_recent(&path)
            ));
            row.add_suffix(&forget);
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

            row.connect_activated(clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.open_recent(&path)
            ));
            imp.recent_list.append(&row);
        }
        imp.recent_group.set_visible(!recents.is_empty());
    }

    /// Ask before forgetting, and say plainly what is and isn't being deleted.
    /// "Have I just deleted my project?" is exactly the fright this app exists
    /// to prevent, and the answer has to be on the screen, not in a tooltip.
    fn forget_recent(&self, path: &Path) {
        let name = project::load(path)
            .map(|project| project.name)
            .unwrap_or_else(|_| t("this project"));
        let folder = project::load(path)
            .ok()
            .and_then(|project| project.source_dir)
            .map(|folder| folder.display().to_string());

        let dialog = adw::AlertDialog::builder()
            .heading(format!("{} {name}?", t("Forget")))
            .body(match folder {
                Some(folder) => format!(
                    "{}\n\n{}\n{folder}",
                    t("This only takes it off the list here. Nothing in the project \
                       folder is deleted — your code, and any manifest already written, \
                       stay exactly as they are:"),
                    t("The folder:")
                ),
                None => t("This only takes it off the list here. Nothing else is deleted."),
            })
            .build();
        dialog.add_response("cancel", &t("Cancel"));
        dialog.add_response("forget", &t("Take it off the list"));
        dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));

        let path = path.to_path_buf();
        dialog.connect_response(
            None,
            clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    if response != "forget" {
                        return;
                    }
                    match project::forget(&path) {
                        Ok(()) => window.refresh_recents(),
                        Err(err) => window.show_error(
                            &t("It couldn't be taken off the list"),
                            &format!("{err:#}"),
                        ),
                    }
                }
            ),
        );
        dialog.present(Some(self));
    }

    // -- Telling the user something went wrong -------------------------------

    /// A cancelled file chooser is not an error; anything else is, and gets a
    /// sentence rather than the toolkit's own wording.
    fn report_dialog_error(&self, err: &glib::Error) {
        if err.matches(gtk::DialogError::Dismissed) || err.matches(gtk::DialogError::Cancelled) {
            return;
        }
        self.show_error(
            &t("The file chooser couldn't be opened"),
            &t("Try again, or restart the app if it keeps happening."),
        );
    }

    fn show_error(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .build();
        dialog.add_response("ok", &t("OK"));
        dialog.set_default_response(Some("ok"));
        dialog.present(Some(self));
    }
}

/// Rows show the value as the title's second line when there is one, and the
/// explanation when there isn't — so an empty field explains itself instead of
/// looking broken.
fn set_value(row: &adw::ActionRow, value: &str, explanation: &str) {
    if value.is_empty() {
        row.set_subtitle(explanation);
        row.add_css_class("dim-label");
    } else {
        row.set_subtitle(value);
        row.remove_css_class("dim-label");
    }
}

/// A path like ".." tells a beginner nothing on its own; the kind of source it
/// is does.
fn describe_source(entry: &packitflat::manifest::SourceEntry) -> String {
    match entry.as_source() {
        Some(source) => format!(
            "{} — {}",
            entry.label(),
            source.kind.explanation().to_lowercase()
        ),
        None => entry.label(),
    }
}

/// "org.gnome.Platform" is a runtime name; "GNOME" is what it is.
fn friendly_runtime(runtime: &str) -> String {
    match runtime {
        "org.gnome.Platform" => "GNOME".to_string(),
        "org.kde.Platform" => "KDE".to_string(),
        "org.freedesktop.Platform" => "Freedesktop".to_string(),
        other => other.to_string(),
    }
}

/// "my-first-app" becomes "My First App": a folder name is a good first guess at
/// what the app is called, once it looks like a name.
fn title_case(text: &str) -> String {
    text.split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first scrolling container in a page, for the harness.
#[cfg(debug_assertions)]
fn find_scroller(root: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Ok(scroller) = widget.clone().downcast::<gtk::ScrolledWindow>() {
            return Some(scroller);
        }
        if let Some(found) = find_scroller(&widget) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// The first expander in a page, for the harness.
#[cfg(debug_assertions)]
/// The carousel on the page that is on top, if that page has one. Only the
/// harness asks: it is how "has the wizard finished moving to the step it was
/// asked for" gets answered without a guessed delay.
#[cfg(debug_assertions)]
fn dev_carousel(window: &PifWindow) -> Option<adw::Carousel> {
    fn find(root: &gtk::Widget) -> Option<adw::Carousel> {
        let mut child = root.first_child();
        while let Some(widget) = child {
            if let Ok(carousel) = widget.clone().downcast::<adw::Carousel>() {
                return Some(carousel);
            }
            if let Some(found) = find(&widget) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    let page = window.imp().nav.visible_page()?.upcast::<gtk::Widget>();
    find(&page)
}

#[cfg(debug_assertions)]
fn visible_expanders(root: &gtk::Widget) -> Vec<adw::ExpanderRow> {
    let mut found = Vec::new();
    collect_expanders(root, &mut found);
    found
}

#[cfg(debug_assertions)]
fn collect_expanders(root: &gtk::Widget, found: &mut Vec<adw::ExpanderRow>) {
    // Only what is on screen. A carousel keeps all eight steps as children and
    // a stack keeps every pane, so an unguarded walk returns step 1's
    // explanation whichever step is showing — which is why this check reported
    // the same scroll distance for all eight of them.
    if let Some(carousel) = root.downcast_ref::<adw::Carousel>() {
        let showing = carousel.position().round().max(0.0) as u32;
        if showing < carousel.n_pages() {
            collect_expanders(&carousel.nth_page(showing), found);
        }
        return;
    }
    if let Some(stack) = root.downcast_ref::<gtk::Stack>() {
        if let Some(page) = stack.visible_child() {
            collect_expanders(&page, found);
        }
        return;
    }

    let mut child = root.first_child();
    while let Some(widget) = child {
        if !widget.is_visible() {
            child = widget.next_sibling();
            continue;
        }
        // Not descending into one: the rows inside an explanation belong to it,
        // and opening the outer row is what shows them.
        if let Ok(expander) = widget.clone().downcast::<adw::ExpanderRow>() {
            found.push(expander);
        } else {
            collect_expanders(&widget, found);
        }
        child = widget.next_sibling();
    }
}

/// A command line, wrapped to run on the host when this copy is sandboxed.
fn host_args(argv: &[&str], sandboxed: bool) -> Vec<String> {
    build::Command {
        argv: argv.iter().map(|part| part.to_string()).collect(),
    }
    .on_host(sandboxed)
    .argv
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}
