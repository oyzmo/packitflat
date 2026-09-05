//! The build page: the checklist, the switches, the running log, and what
//! happened.
//!
//! Everything it *decides* is in `packitflat::build` — which command to run,
//! whether the computer is ready, what a failure means. This file starts
//! processes and puts the answers on screen.
//!
//! The log is deliberately not the main thing on the page: a sentence sits above
//! it saying what is happening, because "Compiling — this is the slow part" is
//! what someone needs during the eight minutes when the log is scrolling past
//! too fast to read.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::clone;
use gtk::{gio, glib};

use packitflat::build::{self, Check, Command, Fix, Options, Option_, Probe, State};
use packitflat::i18n::t;
use packitflat::project::Project;
use packitflat::validate::{self, Severity};

use crate::forms::Handle;
use crate::proc;

/// One of the things offered after a build that worked: what it is called, what
/// it does, and what to do when it is pressed.
type AfterAction = (String, String, Box<dyn Fn(&PifBuild)>);

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/no/oyzmo/PackItFlat/ui/build.ui")]
    pub struct PifBuild {
        #[template_child]
        pub page_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub stage: TemplateChild<gtk::Stack>,
        #[template_child]
        pub cancel_button: TemplateChild<gtk::Button>,

        // Before
        #[template_child]
        pub checks_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub options_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub build_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub command_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub copy_command_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub terminal_button: TemplateChild<gtk::Button>,

        // During
        #[template_child]
        pub status_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        pub log_search: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub log_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub log_scroll: TemplateChild<gtk::ScrolledWindow>,

        // After
        #[template_child]
        pub outcome_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub outcome_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub fix_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub actions_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub actions_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub log_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub copy_log_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub full_log_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub again_button: TemplateChild<gtk::Button>,

        pub handle: RefCell<Option<Handle>>,
        pub options: RefCell<Options>,
        pub probe: RefCell<Probe>,
        /// Everything the build has printed, kept whole so the search can filter
        /// it without losing anything.
        pub log: RefCell<String>,
        pub running: RefCell<Option<Rc<proc::Running>>>,
        /// What the fix button would run, if there is one.
        pub pending_fix: RefCell<Option<Fix>>,
        pub sandboxed: Cell<bool>,
        /// The terminal this computer has, if any, for "Open a terminal".
        pub terminal: RefCell<Option<&'static build::Terminal>>,
        /// The build command as it would be typed, or `None` while there is no
        /// manifest name to put in it. Copying, the terminal button and the dev
        /// check all read it from here rather than off the row's subtitle, which
        /// is a sentence rather than a command whenever this is `None`.
        pub command_text: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PifBuild {
        const NAME: &'static str = "PifBuild";
        type Type = super::PifBuild;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PifBuild {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PifBuild {}
    impl NavigationPageImpl for PifBuild {}
}

glib::wrapper! {
    pub struct PifBuild(ObjectSubclass<imp::PifBuild>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PifBuild {
    /// Open the build page over whatever page asked for it, on the same project.
    pub fn push_from(from: &impl IsA<gtk::Widget>, project: Rc<RefCell<Project>>) {
        if let Some(view) = from.as_ref().parent().and_downcast::<adw::NavigationView>() {
            view.push(&PifBuild::new(project));
        }
    }

    pub fn new(project: Rc<RefCell<Project>>) -> Self {
        let page: Self = glib::Object::builder().build();
        let handle = Handle::new(project, || {});
        page.imp().handle.replace(Some(handle));
        page.imp()
            .sandboxed
            .set(std::path::Path::new("/.flatpak-info").exists());

        page.load_options();
        page.refresh_command();
        page.check_the_computer();
        page
    }

    fn handle(&self) -> Handle {
        self.imp()
            .handle
            .borrow()
            .clone()
            .expect("the build page is only built with a project")
    }

    // -- wiring --------------------------------------------------------------

    fn setup(&self) {
        let imp = self.imp();

        imp.build_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.start_build()
        ));
        imp.again_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                page.imp().stage.set_visible_child_name("ready");
                page.check_the_computer();
            }
        ));
        imp.cancel_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                if let Some(running) = page.imp().running.borrow().as_ref() {
                    running.cancel();
                }
                page.imp()
                    .status_banner
                    .set_title(&t("Stopping the build…"));
            }
        ));

        imp.copy_command_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |button| {
                page.clipboard()
                    .set_text(&page.command_text());
                flash(button, &t("Copied"));
            }
        ));
        imp.copy_log_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |button| {
                page.clipboard().set_text(&page.imp().log.borrow());
                flash(button, &t("Copied"));
            }
        ));

        imp.terminal_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.open_in_terminal()
        ));

        imp.log_search.connect_search_changed(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| page.render_log()
        ));

        imp.fix_button.connect_clicked(clone!(
            #[weak(rename_to = page)]
            self,
            move |_| {
                let fix = page.imp().pending_fix.borrow().clone();
                if let Some(fix) = fix {
                    page.apply(&fix);
                }
            }
        ));
    }

    // -- before the build ----------------------------------------------------

    fn load_options(&self) {
        let imp = self.imp();
        crate::forms::clear(&imp.options_list);

        for option in Option_::ALL {
            let option = *option;
            let row = adw::SwitchRow::builder()
                .title(t(option.label()))
                .subtitle(t(option.explanation()))
                .subtitle_lines(0)
                .active(option.get(&imp.options.borrow()))
                .build();
            row.connect_active_notify(clone!(
                #[weak(rename_to = page)]
                self,
                move |row| {
                    option.set(&mut page.imp().options.borrow_mut(), row.is_active());
                    page.refresh_command();
                }
            ));
            imp.options_list.append(&row);
        }
    }

    fn refresh_command(&self) {
        let imp = self.imp();
        let app_id = self
            .handle()
            .read(|project| project.manifest.app_id.trim().to_string());

        // The manifest is named after the app ID, so without one the command
        // ends in a bare ".yml" — a line that looks copyable, runs, and fails on
        // a file that was never going to exist. The checklist above already says
        // what to do about it, so this says why the command isn't here yet.
        if app_id.is_empty() {
            imp.command_text.replace(None);
            imp.command_row.set_subtitle(&t(
                "The manifest is named after the app ID, and that is still empty. \
                 Fill it in and the command to run appears here.",
            ));
            imp.copy_command_button.set_sensitive(false);
            imp.terminal_button.set_sensitive(false);
            return;
        }

        // The whole job, not just the build: this string is what Copy copies
        // and what "Open a terminal" runs, and it used to stop before the file
        // was packed.
        let command = build::build_line(&app_id, &imp.options.borrow(), imp.probe.borrow().flathub);
        imp.command_text.replace(Some(command.clone()));
        imp.command_row.set_subtitle(&command);
        imp.copy_command_button.set_sensitive(true);
        imp.terminal_button.set_sensitive(true);
    }

    /// The build command as it would be typed, or the empty string while the
    /// manifest has no name.
    fn command_text(&self) -> String {
        self.imp().command_text.borrow().clone().unwrap_or_default()
    }

    /// Run the same build in a terminal window instead. Some people would rather
    /// watch it there — and when the app itself can't start a build, this is the
    /// way that still works.
    fn open_in_terminal(&self) {
        let imp = self.imp();
        let Some(terminal) = *imp.terminal.borrow() else {
            return;
        };
        let Some(folder) = self.handle().read(|project| project.source_dir.clone()) else {
            return;
        };

        let command = self.command_text();
        if command.is_empty() {
            return;
        }
        let argv = build::terminal_argv(terminal, &folder.display().to_string(), &command);
        let argv = Command { argv }.on_host(imp.sandboxed.get()).argv;

        // Started and let go: it is the terminal's window now, not this app's.
        let spawned = gio::Subprocess::newv(
            &argv.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>(),
            gio::SubprocessFlags::NONE,
        );
        if spawned.is_err() {
            self.tell(
                &t("The terminal couldn't be opened"),
                &format!(
                    "{}\n\n{}",
                    t("Copy the command instead and paste it into a terminal yourself:"),
                    command
                ),
            );
        }
    }

    /// Ask the computer what it has, then judge it. Every question is a short
    /// command, and none of them changes anything.
    fn check_the_computer(&self) {
        glib::spawn_future_local(clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let sandboxed = page.imp().sandboxed.get();
                let handle = page.handle();

                let flatpak_present = run_ok(&["flatpak", "--version"], sandboxed).await;
                let builder_present = run_ok(&["flatpak-builder", "--version"], sandboxed).await;

                // The options column is what says which installation the remote
                // is in, and that decides the flag every download here needs.
                // Asking only for the name was the bug: Flathub set up for the
                // whole computer looked the same as Flathub set up for this
                // user, and the download was then aimed at the user's, which on
                // Fedora has no Flathub in it at all.
                let remotes =
                    run_text(&["flatpak", "remotes", "--columns=name,options"], sandboxed).await;
                let flathub = build::remote_scope(&remotes, "flathub");

                let installed = run_text(
                    &["flatpak", "list", "--columns=application,branch"],
                    sandboxed,
                )
                .await;
                let installed_refs = installed_refs(&installed);

                let sandboxed_without_host =
                    sandboxed && !run_ok(&["true"], true).await;

                let (blocking_issues, manifest_written) = handle.read(|project| {
                    let blocking = validate::project(project)
                        .into_iter()
                        .filter(|issue| issue.severity == Severity::Error)
                        .collect();
                    let written = manifest_path(project).is_some_and(|path| path.exists());
                    (blocking, written)
                });

                // Which terminal this computer has, for "Open a terminal".
                let mut found = None;
                for terminal in build::TERMINALS {
                    if run_ok(&[terminal.program, "--version"], sandboxed).await
                        || run_ok(&["which", terminal.program], sandboxed).await
                    {
                        found = Some(terminal);
                        break;
                    }
                }
                page.imp().terminal.replace(found);
                page.imp().terminal_button.set_visible(found.is_some());
                if let Some(terminal) = found {
                    page.imp().terminal_button.set_label(&format!(
                        "{} {}",
                        t("Open"),
                        t(terminal.label)
                    ));
                }

                page.imp().probe.replace(Probe {
                    flatpak_present,
                    builder_present,
                    flathub,
                    installed_refs,
                    sandboxed_without_host,
                    blocking_issues,
                    manifest_written,
                });
                page.show_checks();
            }
        ));
    }

    fn show_checks(&self) {
        let imp = self.imp();
        crate::forms::clear(&imp.checks_list);

        let checks = self
            .handle()
            .read(|project| build::preflight(project, &imp.probe.borrow()));

        for check in &checks {
            imp.checks_list.append(&self.check_row(check));
        }

        let ready = build::can_build(&checks);
        imp.build_button.set_sensitive(ready);
        imp.build_button.set_tooltip_text(Some(&if ready {
            t("Start the build")
        } else {
            t("Something above has to be sorted out first")
        }));
    }

    fn check_row(&self, check: &Check) -> adw::ActionRow {
        let row = adw::ActionRow::builder()
            .title(&check.title)
            .subtitle(&check.detail)
            .title_lines(0)
            .subtitle_lines(0)
            .build();

        let (icon, css) = match check.state {
            State::Ready => ("object-select-symbolic", None),
            State::Fixable => ("dialog-information-symbolic", Some("note-info")),
            State::Blocked => ("dialog-warning-symbolic", Some("note-warning")),
        };
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        if let Some(css) = css {
            row.add_css_class(css);
        }

        if let Some(fix) = &check.fix {
            let label = match fix {
                Fix::Run { label, .. } | Fix::Copy { label, .. } | Fix::Elsewhere { label, .. } => {
                    label.clone()
                }
            };
            let button = gtk::Button::builder()
                .label(label)
                .valign(gtk::Align::Center)
                .build();
            let fix = fix.clone();
            button.connect_clicked(clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.apply(&fix)
            ));
            row.add_suffix(&button);
        }

        row
    }

    /// Do something about a failed check: run the command, copy it, or say where
    /// in the app the answer lives.
    fn apply(&self, fix: &Fix) {
        match fix {
            Fix::Run { command, label } => {
                self.run_command(
                    command.clone(),
                    &format!("{} — {}", t("Running"), label.to_lowercase()),
                );
            }
            Fix::Copy { command, .. } => {
                self.clipboard().set_text(&command.as_typed());
                self.tell(
                    &t("Copied"),
                    &format!(
                        "{}\n\n{}",
                        command.as_typed(),
                        t("Paste it into a terminal, then come back and try again.")
                    ),
                );
            }
            // Not a message telling someone where to go: it goes there.
            Fix::Elsewhere { label, field } => self.go_to(*field, label),
        }
    }

    /// Leave the build page and land on the thing that needs fixing — the step
    /// in the guided setup, or the pane in the editor, whichever the user came
    /// from. Telling somebody to "go and fix it" and leaving them to find it is
    /// exactly the sort of thing this app is supposed to stop doing.
    fn go_to(&self, field: Option<validate::Field>, label: &str) {
        let Some(view) = self.parent().and_downcast::<adw::NavigationView>() else {
            return;
        };
        view.pop();

        let Some(page) = view.visible_page() else {
            return;
        };
        if let Some(wizard) = page.downcast_ref::<crate::wizard::PifWizard>() {
            wizard.go_to_step(field.map(|field| field.step()).unwrap_or(7));
        } else if let Some(editor) = page.downcast_ref::<crate::editor::PifEditor>() {
            editor.show_field(field);
        } else {
            // The project summary: nothing to jump to, so say where to look.
            self.tell(
                label,
                &t("Open the project — in the guided steps or the editor — and it will \
                    be waiting there."),
            );
        }
    }

    // -- the build itself ----------------------------------------------------

    fn start_build(&self) {
        let app_id = self.handle().read(|project| project.manifest.app_id.trim().to_string());
        let options = self.imp().options.borrow().clone();
        // Packing the file is part of pressing "Build", not a second thing to
        // remember: the two run as one shell line so the log reads as one job.
        // The same line the row shows and the terminal button runs.
        let line = build::build_line(&app_id, &options, self.imp().probe.borrow().flathub);
        let command = Command {
            argv: vec!["sh".into(), "-c".into(), line],
        };

        self.run_command(command, &t("Starting the build…"));
    }

    /// Run something long, in the project folder, with its output on screen.
    fn run_command(&self, command: Command, status: &str) {
        let imp = self.imp();
        let Some(folder) = self.handle().read(|project| project.source_dir.clone()) else {
            return self.tell(
                &t("There's nowhere to build"),
                &t("This project isn't linked to a folder yet."),
            );
        };

        let command = command.on_host(imp.sandboxed.get());
        // Run where the manifest is: every path in it is relative to that folder.
        let argv: Vec<String> = ["sh".into(), "-c".into()]
            .into_iter()
            .chain(std::iter::once(format!(
                "cd {} && exec {}",
                shell_quote(&folder.display().to_string()),
                command.as_typed()
            )))
            .collect();

        imp.log.replace(String::new());
        self.render_log();
        imp.status_banner.set_title(status);
        imp.spinner.set_spinning(true);
        imp.cancel_button.set_visible(true);
        imp.stage.set_visible_child_name("running");

        let started = proc::stream(
            &argv,
            clone!(
                #[weak(rename_to = page)]
                self,
                move |line| page.take_line(&line)
            ),
            clone!(
                #[weak(rename_to = page)]
                self,
                move |code, cancelled| page.finished(code, cancelled)
            ),
        );

        match started {
            Some(running) => {
                imp.running.replace(Some(running));
            }
            None => {
                imp.spinner.set_spinning(false);
                imp.cancel_button.set_visible(false);
                self.finished(None, false);
            }
        }
    }

    fn take_line(&self, line: &str) {
        let imp = self.imp();
        imp.log.borrow_mut().push_str(line);
        if !line.ends_with('\n') {
            imp.log.borrow_mut().push('\n');
        }

        if let Some(status) = build::status_line(line) {
            imp.status_banner.set_title(&t(&status));
        }
        self.render_log();
    }

    /// The log, filtered by whatever is in the search box. The whole thing is
    /// always kept — searching hides lines, it never throws them away.
    fn render_log(&self) {
        let imp = self.imp();
        let needle = imp.log_search.text().to_lowercase();
        let log = imp.log.borrow();

        let text = if needle.trim().is_empty() {
            log.clone()
        } else {
            log.lines()
                .filter(|line| line.to_lowercase().contains(needle.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        imp.log_view.buffer().set_text(&text);
        imp.full_log_view.buffer().set_text(&log);

        // Follow the end, the way a terminal does, unless someone is searching.
        if needle.trim().is_empty() {
            if let Some(adjustment) = imp.log_scroll.vadjustment().into() {
                let adjustment: gtk::Adjustment = adjustment;
                glib::idle_add_local_once(move || {
                    adjustment.set_value(adjustment.upper() - adjustment.page_size());
                });
            }
        }
    }

    fn finished(&self, code: Option<i32>, cancelled: bool) {
        let imp = self.imp();
        imp.spinner.set_spinning(false);
        imp.cancel_button.set_visible(false);
        imp.running.replace(None);
        imp.stage.set_visible_child_name("finished");
        crate::forms::clear(&imp.actions_list);

        let log = imp.log.borrow().clone();

        if cancelled {
            imp.outcome_icon.set_icon_name(Some("process-stop-symbolic"));
            imp.outcome_row.set_title(&t("The build was stopped"));
            imp.outcome_row.set_subtitle(&t(
                "Nothing was installed. What it had already built is still in the \
                 working folder, so building again picks up where it can.",
            ));
            imp.outcome_row.remove_css_class("note-warning");
            imp.fix_button.set_visible(false);
            imp.actions_group.set_visible(false);
            return;
        }

        let Some(code) = code else {
            imp.outcome_icon.set_icon_name(Some("dialog-warning-symbolic"));
            imp.outcome_row.set_title(&t("The build couldn't be started"));
            imp.outcome_row.set_subtitle(&t(
                "The command couldn't be run at all. That usually means \
                 flatpak-builder isn't installed.",
            ));
            imp.outcome_row.add_css_class("note-warning");
            imp.fix_button.set_visible(false);
            imp.actions_group.set_visible(false);
            return;
        };

        if code == 0 {
            imp.outcome_icon.set_icon_name(Some("object-select-symbolic"));
            imp.outcome_row.remove_css_class("note-warning");
            imp.outcome_row.set_title(&t("It built"));
            let app_id = self
                .handle()
                .read(|project| project.manifest.app_id.trim().to_string());
            let made_bundle = imp.options.borrow().make_bundle && !app_id.is_empty();
            imp.outcome_row.set_subtitle(&if made_bundle {
                format!(
                    "{} {} {}",
                    t("The finished app is in"),
                    build::bundle_file(&app_id),
                    t(
                        "next to the manifest — one file, which anyone can install by \
                         double-clicking it. Nothing has been added to this computer \
                         unless you asked for it."
                    )
                )
            } else {
                t("The app is finished and ready to install. Nothing has been added to \
                   this computer unless you asked for it.")
            });
            imp.fix_button.set_visible(false);
            self.show_after_actions();
            return;
        }

        // Failed: never the raw log first. The scope goes in so that a fix which
        // offers to download something aims at the installation this computer
        // actually keeps Flathub in.
        let flathub = imp.probe.borrow().flathub;
        let diagnosis = self
            .handle()
            .read(|project| build::diagnose(project, &log, code, flathub));
        imp.outcome_icon.set_icon_name(Some("dialog-warning-symbolic"));
        imp.outcome_row.add_css_class("note-warning");
        imp.outcome_row.set_title(&diagnosis.headline);
        imp.outcome_row
            .set_subtitle(&format!("{}\n\n{}", diagnosis.detail, diagnosis.excerpt));

        match diagnosis.fix {
            Some(fix) => {
                let label = match &fix {
                    Fix::Run { label, .. } | Fix::Copy { label, .. } | Fix::Elsewhere { label, .. } => {
                        label.clone()
                    }
                };
                imp.fix_button.set_label(&label);
                imp.fix_button.set_visible(true);
                imp.fix_button.add_css_class("suggested-action");
                imp.pending_fix.replace(Some(fix));
            }
            None => {
                imp.fix_button.set_visible(false);
                imp.pending_fix.replace(None);
            }
        }

        imp.actions_group.set_visible(false);
        imp.log_expander.set_expanded(true);
    }

    /// What someone might want after a build that worked.
    fn show_after_actions(&self) {
        let imp = self.imp();
        let (app_id, folder) = self
            .handle()
            .read(|project| (project.manifest.app_id.clone(), project.source_dir.clone()));
        let options = imp.options.borrow().clone();

        let mut rows: Vec<AfterAction> = Vec::new();

        rows.push((
            t("Install it on this computer"),
            t("Adds it to the menu like any other app. You can remove it again with \
               Software, or with flatpak uninstall."),
            Box::new({
                let app_id = app_id.clone();
                move |page: &PifBuild| {
                    page.run_command(build::install_command(&app_id), &t("Installing…"))
                }
            }),
        ));

        rows.push((
            t("Run it"),
            t("Starts the app. It has to be installed first."),
            Box::new({
                let app_id = app_id.clone();
                move |page: &PifBuild| {
                    page.run_command(build::run_command(&app_id), &t("Starting the app…"))
                }
            }),
        ));

        // Only worth offering when the build wasn't asked to do it already —
        // otherwise the file is sitting there and this row is a rebuild.
        if !options.make_bundle {
            rows.push((
                t("Make a single file to share"),
                format!(
                    "{} {}. {}",
                    t("Puts the whole app into"),
                    build::bundle_file(&app_id),
                    t("Anyone can install that with a double-click.")
                ),
                Box::new({
                    let app_id = app_id.clone();
                    let options = options.clone();
                    move |page: &PifBuild| page.export(&app_id, &options)
                }),
            ));
        }

        if let Some(folder) = folder {
            rows.push((
                t("Open the project folder"),
                folder.display().to_string(),
                Box::new(move |_page: &PifBuild| {
                    let uri = gio::File::for_path(&folder).uri();
                    let _ = gio::AppInfo::launch_default_for_uri(&uri, gio::AppLaunchContext::NONE);
                }),
            ));
        }

        for (title, subtitle, action) in rows {
            let row = adw::ActionRow::builder()
                .title(title)
                .subtitle(subtitle)
                .subtitle_lines(0)
                .activatable(true)
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            row.connect_activated(clone!(
                #[weak(rename_to = page)]
                self,
                move |_| action(&page)
            ));
            imp.actions_list.append(&row);
        }

        imp.actions_group.set_visible(true);
    }

    /// Exporting is two commands; they are run as one shell line so the log
    /// reads as a single operation.
    fn export(&self, app_id: &str, options: &Options) {
        let commands = build::export_commands(app_id, options, self.imp().probe.borrow().flathub);
        let joined = commands
            .iter()
            .map(|command| command.as_typed())
            .collect::<Vec<_>>()
            .join(" && ");

        self.run_command(
            Command {
                argv: vec!["sh".into(), "-c".into(), joined],
            },
            &t("Packing it into one file…"),
        );
    }

    // -- odds and ends -------------------------------------------------------

    fn tell(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .build();
        dialog.add_response("ok", &t("OK"));
        dialog.set_default_response(Some("ok"));
        dialog.present(Some(self));
    }

    /// Only the screenshot harness needs this: it drives the page with a
    /// stand-in command so the running and finished states can be photographed
    /// on a machine with no flatpak-builder.
    #[cfg(debug_assertions)]
    pub fn dev_run(&self, script: &str) {
        self.run_command(
            Command {
                argv: vec!["sh".into(), "-c".into(), script.to_string()],
            },
            &t("Starting…"),
        );
    }

    /// Report what the "Open a terminal" offer came to on this computer, and
    /// exactly what pressing it would run.
    #[cfg(debug_assertions)]
    pub fn dev_terminal_check(&self) {
        let page = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            let imp = page.imp();
            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("terminal-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };

            let terminal = *imp.terminal.borrow();
            check("a terminal was found on this computer", terminal.is_some());
            check(
                "the button offers it by name",
                imp.terminal_button.is_visible()
                    && imp
                        .terminal_button
                        .label()
                        .is_some_and(|label| label.starts_with("Open ")),
            );

            let command = page.command_text();
            if command.is_empty() {
                // No app ID yet, so no manifest to name and nothing to run. What
                // matters then is that the page offers nothing: an argv built
                // around an empty command comes out as `cd … && ;`, which is a
                // shell syntax error rather than a command line.
                eprintln!("terminal-check:      there is no command yet");
                check(
                    "with nothing to run, the terminal button is insensitive",
                    !imp.terminal_button.is_sensitive(),
                );
                check(
                    "and so is the copy button",
                    !imp.copy_command_button.is_sensitive(),
                );
            } else if let Some(terminal) = terminal {
                let folder = page
                    .handle()
                    .read(|project| project.source_dir.clone())
                    .unwrap_or_default();
                let argv = build::terminal_argv(terminal, &folder.display().to_string(), &command);
                eprintln!("terminal-check: would run {}", argv.join(" "));

                check(
                    "it runs the same command the page shows",
                    argv.last().is_some_and(|script| script.contains(&command)),
                );
                check(
                    "in the project folder",
                    argv.iter()
                        .any(|part| part.contains(&folder.display().to_string())),
                );
                check(
                    "and the window is kept open afterwards",
                    argv.last().is_some_and(|script| script.contains("read _")),
                );
            }

            eprintln!(
                "terminal-check: {}",
                if ok { "all checks passed" } else { "FAILURES ABOVE" }
            );
            if let Some(window) = page.root().and_downcast::<gtk::Window>() {
                window.close();
            }
            if !ok {
                std::process::exit(1);
            }
        });
    }

    /// Drive a whole failing build and check what the page did with it. The
    /// translation itself is unit-tested in `packitflat::build`; this checks the
    /// part unit tests can't reach — that output streams in, that the exit code
    /// arrives, and that what lands on screen is the sentence rather than the
    /// log.
    #[cfg(debug_assertions)]
    pub fn dev_build_check(&self) {
        // A stand-in for flatpak-builder failing the way it does when the
        // runtime is missing.
        self.dev_run(
            "echo 'Downloading sources'; \
             echo 'Building module cleaner'; \
             echo 'error: Failed to init: Unable to find sdk org.gnome.Sdk version 50' >&2; \
             exit 1",
        );

        let page = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            let imp = page.imp();
            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("build-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };

            check(
                "the log streamed in",
                imp.log.borrow().contains("Building module cleaner"),
            );
            check(
                "the page moved on when the build ended",
                imp.stage.visible_child_name().as_deref() == Some("finished"),
            );

            let title = imp.outcome_row.title().to_string();
            check(
                "the failure is a sentence, not a log line",
                title == "The runtime this app builds on isn't installed",
            );
            check(
                "the raw error is kept, but underneath",
                imp.outcome_row
                    .subtitle()
                    .is_some_and(|subtitle| subtitle.contains("Unable to find sdk")),
            );
            check(
                "there is something to press about it",
                imp.fix_button.is_visible()
                    && imp.fix_button.label().is_some_and(|label| label == "Download them"),
            );
            check(
                "the whole log is still there to read",
                imp.full_log_view.buffer().end_iter().offset() > 0,
            );

            eprintln!(
                "build-check: {}",
                if ok { "all checks passed" } else { "FAILURES ABOVE" }
            );
            if let Some(window) = page.root().and_downcast::<gtk::Window>() {
                window.close();
            }
            if !ok {
                std::process::exit(1);
            }
        });
    }
}

fn manifest_path(project: &Project) -> Option<PathBuf> {
    let app_id = project.manifest.app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    project
        .source_dir
        .as_ref()
        .map(|folder| folder.join(format!("{app_id}.yml")))
}

/// `flatpak list` output into the `id//branch` refs the preflight compares
/// against.
fn installed_refs(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, branch)| (id.trim(), branch.trim()))
        .filter(|(id, branch)| !id.is_empty() && !branch.is_empty())
        .flat_map(|(id, branch)| [format!("{id}//{branch}"), id.to_string()])
        .collect()
}

async fn run_ok(argv: &[&str], sandboxed: bool) -> bool {
    let command = Command {
        argv: argv.iter().map(|part| part.to_string()).collect(),
    }
    .on_host(sandboxed);
    proc::succeeds(&command.argv).await
}

async fn run_text(argv: &[&str], sandboxed: bool) -> String {
    let command = Command {
        argv: argv.iter().map(|part| part.to_string()).collect(),
    }
    .on_host(sandboxed);
    proc::output(&command.argv)
        .await
        .map(|(text, _)| text)
        .unwrap_or_default()
}

/// Single quotes, the way a shell wants them, so a folder with a space in it
/// doesn't turn into two arguments.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn flash(button: &gtk::Button, text: &str) {
    let original = button.label().map(|label| label.to_string());
    let had_icon = button.icon_name().map(|name| name.to_string());
    button.set_label(text);

    let button = button.clone();
    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
        match (original, had_icon) {
            (Some(label), _) => button.set_label(&label),
            (None, Some(icon)) => button.set_icon_name(&icon),
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_refs_are_matched_with_and_without_a_version() {
        let refs = installed_refs("org.gnome.Platform\t50\norg.freedesktop.Sdk.Extension.rust-stable\t25.08\n");
        assert!(refs.contains(&"org.gnome.Platform//50".to_string()));
        // Extensions are asked for without a version, so both forms are kept.
        assert!(refs.contains(&"org.freedesktop.Sdk.Extension.rust-stable".to_string()));
        assert!(installed_refs("nonsense\n").is_empty());
    }

    #[test]
    fn folders_with_spaces_survive_the_shell() {
        assert_eq!(shell_quote("/home/me/my project"), "'/home/me/my project'");
        assert_eq!(shell_quote("it's here"), "'it'\\''s here'");
    }
}
