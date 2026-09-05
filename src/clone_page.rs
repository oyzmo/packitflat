//! The dialog behind "Start from a Git address".
//!
//! Everything it decides is in `packitflat::git` — what a pasted address means,
//! where the code may go, what to run, what a failure was about. This file runs
//! git, shows what it is doing, and hands the finished project back.
//!
//! The clone is shallow and happens once: the app needs the code on disk to see
//! what kind of project it is, to read its lock file, and to find out what the
//! program is called. The manifest it produces points at the repository, pinned
//! to the commit that was actually checked out.

use std::cell::RefCell;
use std::path::PathBuf;


use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::clone;
use gtk::glib;

use packitflat::git;
use packitflat::i18n::t;
use packitflat::project::Project;

use crate::proc;

mod imp {
    use super::*;

    // No Debug: the struct holds the callback that hands the finished project
    // back, and a boxed closure has nothing to print.
    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/no/oyzmo/PackItFlat/ui/clone.ui")]
    pub struct PifClone {
        #[template_child]
        pub stage: TemplateChild<gtk::Stack>,
        #[template_child]
        pub url_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub url_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub url_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub url_fix_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub version_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub folder_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub folder_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub clone_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        pub status_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub log_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub failure_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub failure_log_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub again_button: TemplateChild<gtk::Button>,

        /// Where the code will go. Followed from the address unless the user
        /// picks somewhere themselves.
        pub folder: RefCell<Option<PathBuf>>,
        pub folder_chosen: RefCell<bool>,
        pub log: RefCell<String>,
        /// What to do with the finished project.
        #[allow(clippy::type_complexity)]
        pub on_ready: RefCell<Option<Box<dyn Fn(Project)>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PifClone {
        const NAME: &'static str = "PifClone";
        type Type = super::PifClone;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PifClone {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PifClone {}
    impl AdwDialogImpl for PifClone {}
}

glib::wrapper! {
    pub struct PifClone(ObjectSubclass<imp::PifClone>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PifClone {
    pub fn new(on_ready: impl Fn(Project) + 'static) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.imp().on_ready.replace(Some(Box::new(on_ready)));
        dialog
    }

    fn setup(&self) {
        let imp = self.imp();

        imp.url_entry.connect_changed(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.check_url()
        ));
        imp.url_entry.connect_activate(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.start()
        ));

        // "That's a link to a page" comes with the address it should have been.
        imp.url_fix_button.connect_clicked(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| {
                let suggestion = dialog.imp().url_fix_button.tooltip_text();
                if let Some(suggestion) = suggestion {
                    dialog.imp().url_entry.set_text(&suggestion);
                }
            }
        ));

        imp.folder_button.connect_clicked(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.choose_folder()
        ));
        imp.clone_button.connect_clicked(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| dialog.start()
        ));
        imp.again_button.connect_clicked(clone!(
            #[weak(rename_to = dialog)]
            self,
            move |_| {
                dialog.imp().stage.set_visible_child_name("form");
                dialog.check_url();
            }
        ));

        self.check_url();
    }

    /// Everything the form knows how to say about what has been typed so far.
    fn check_url(&self) {
        let imp = self.imp();
        let typed = imp.url_entry.text().to_string();

        let (message, suggestion, ok) = match git::normalise_url(&typed) {
            Ok(url) => {
                if !*imp.folder_chosen.borrow() {
                    let folder = git::repo_name(&url);
                    imp.folder.replace(default_folder(&folder));
                }
                (None, None, true)
            }
            Err(git::UrlProblem::Empty) => (None, None, false),
            Err(problem @ git::UrlProblem::PageNotRepository(_)) => {
                let suggestion = match &problem {
                    git::UrlProblem::PageNotRepository(url) => Some(url.clone()),
                    _ => None,
                };
                (Some(problem.message()), suggestion, false)
            }
            Err(problem) => (Some(problem.message()), None, false),
        };

        match message {
            Some(message) => {
                imp.url_status.set_title(&message);
                imp.url_status.set_visible(true);
                imp.url_status.add_css_class("note-warning");
                imp.url_icon.set_icon_name(Some("dialog-warning-symbolic"));
            }
            None => imp.url_status.set_visible(false),
        }
        match suggestion {
            Some(suggestion) => {
                imp.url_fix_button.set_visible(true);
                imp.url_fix_button.set_tooltip_text(Some(&suggestion));
            }
            None => imp.url_fix_button.set_visible(false),
        }

        let folder = imp.folder.borrow().clone();
        imp.folder_row.set_subtitle(&match &folder {
            Some(path) => path.display().to_string(),
            None => t("Paste an address and a folder is suggested for you."),
        });

        let folder_ok = folder
            .as_deref()
            .map(git::check_destination)
            .transpose();
        match &folder_ok {
            Ok(_) => imp.folder_row.remove_css_class("note-warning"),
            Err(problem) => {
                imp.folder_row.set_subtitle(&format!(
                    "{}\n{}",
                    folder.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                    problem.message()
                ));
                imp.folder_row.add_css_class("note-warning");
            }
        }

        imp.clone_button
            .set_sensitive(ok && folder.is_some() && folder_ok.is_ok());
    }

    fn choose_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title(t("Where should the code go?"))
            .accept_label(t("Use this folder"))
            .modal(true)
            .build();

        glib::spawn_future_local(clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let window = page.root().and_downcast::<gtk::Window>();
                let Ok(file) = dialog.select_folder_future(window.as_ref()).await else {
                    return;
                };
                let Some(path) = file.path() else { return };

                // A folder the user picked is used as it is; the repository's
                // own name is only a suggestion when they haven't.
                page.imp().folder.replace(Some(path));
                page.imp().folder_chosen.replace(true);
                page.check_url();
            }
        ));
    }

    // -- fetching ------------------------------------------------------------

    fn start(&self) {
        let imp = self.imp();
        let Ok(url) = git::normalise_url(&imp.url_entry.text()) else {
            return;
        };
        let Some(folder) = imp.folder.borrow().clone() else {
            return;
        };
        if git::check_destination(&folder).is_err() {
            return;
        }

        let version = imp.version_entry.text().to_string();
        let argv = git::clone_command(&url, Some(&version), &folder);

        imp.log.replace(String::new());
        imp.log_view.buffer().set_text("");
        imp.status_label.set_label(&t("Fetching the code…"));
        imp.spinner.set_spinning(true);
        imp.stage.set_visible_child_name("working");

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
                move |code, _cancelled| page.fetched(code, url, version, folder)
            ),
        );

        if started.is_none() {
            self.failed(&t("Git isn't installed on this computer. Your distribution \
                            packages it as “git”."));
        }
    }

    fn take_line(&self, line: &str) {
        let imp = self.imp();
        imp.log.borrow_mut().push_str(line);
        if !line.ends_with('\n') {
            imp.log.borrow_mut().push('\n');
        }
        if let Some(status) = git::status_line(line) {
            imp.status_label.set_label(&t(&status));
        }
        imp.log_view.buffer().set_text(&imp.log.borrow());
    }

    fn fetched(&self, code: Option<i32>, url: String, version: String, folder: PathBuf) {
        let imp = self.imp();
        imp.spinner.set_spinning(false);

        if code != Some(0) {
            let log = imp.log.borrow().clone();
            return self.failed(&git::diagnose(&log));
        }

        // Which revision this actually is — the thing the manifest pins.
        imp.status_label.set_label(&t("Looking at what came down…"));
        glib::spawn_future_local(clone!(
            #[weak(rename_to = page)]
            self,
            async move {
                let commit = proc::output(&git::head_command(&folder))
                    .await
                    .and_then(|(text, code)| (code == 0).then_some(text))
                    .and_then(|text| git::parse_head(&text));

                let Some(commit) = commit else {
                    return page.failed(&t(
                        "The code was fetched, but git couldn't say which revision it \
                         is. Try starting from the folder instead — it's already been \
                         copied.",
                    ));
                };

                page.ready(&url, &version, &folder, &commit);
            }
        ));
    }

    /// Detection first — that is why the code was fetched — then point the
    /// manifest back at the repository.
    fn ready(&self, url: &str, version: &str, folder: &std::path::Path, commit: &str) {
        let (mut project, detection) = Project::from_folder(folder);

        git::use_repository(
            &mut project,
            url,
            commit,
            Some(version).filter(|version| !version.trim().is_empty()),
        );
        project.detected = Some(format!(
            "{} {}",
            detection.kind.title(),
            t("fetched from its repository")
        ));

        self.close();
        if let Some(on_ready) = self.imp().on_ready.borrow().as_ref() {
            on_ready(project);
        }
    }

    fn failed(&self, message: &str) {
        let imp = self.imp();
        imp.spinner.set_spinning(false);
        imp.failure_row.set_title(message);
        imp.failure_row
            .set_subtitle(&t("Nothing has been changed on this computer."));
        imp.failure_log_view
            .buffer()
            .set_text(&imp.log.borrow());
        imp.stage.set_visible_child_name("failed");
    }
}

impl PifClone {
    /// Fill the form in and press the button, for the harness. Everything after
    /// this is the same code a person's click runs.
    #[cfg(debug_assertions)]
    pub fn dev_start(&self, url: &str, folder: &std::path::Path) {
        let imp = self.imp();
        imp.url_entry.set_text(url);
        imp.folder.replace(Some(folder.to_path_buf()));
        imp.folder_chosen.replace(true);
        self.check_url();
        self.start();
    }

    /// What the dialog is showing, so a failure can be reported rather than
    /// silently photographed.
    #[cfg(debug_assertions)]
    pub fn dev_failure(&self) -> Option<String> {
        (self.imp().stage.visible_child_name().as_deref() == Some("failed"))
            .then(|| self.imp().failure_row.title().to_string())
    }
}

/// `~/Projects/<repository>`, which is where this sort of thing usually goes.
fn default_folder(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let projects = home.join("Projects");
    Some(if projects.is_dir() {
        projects.join(name)
    } else {
        home.join(name)
    })
}
