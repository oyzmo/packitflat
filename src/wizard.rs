//! The guided setup: five steps over one project, ending in a written manifest.
//!
//! The project is shared with the window and the editor through a
//! [`forms::Handle`] — one value, three views, which is what makes "never lose
//! data when switching" true by construction rather than by careful syncing.
//!
//! Every widget change runs the same cycle: `collect` reads the widgets into the
//! project, `refresh` re-validates and repaints everything derived from it. The
//! handle is muted while widgets are being filled, so the two don't chase each
//! other.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::clone;
use gtk::{gio, glib};

use packitflat::i18n::t;
use packitflat::manifest::{BuildSystem, SourceKind};
use packitflat::project::Project;
use packitflat::validate::{self, Issues, Severity};
use packitflat::{generate, runtimes, spdx};

use crate::forms::{self, Handle};
use crate::panes;

/// The step titles, in order. Also how many steps there are.
const STEPS: &[&str] = &[
    "The basics",
    "What it runs on",
    "Where the code is",
    "How it's built",
    "What it downloads",
    "What it may do",
    "How it looks",
    "Review and write",
];

mod imp {
    use super::*;
    use std::cell::Cell;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/no/oyzmo/PackItFlat/ui/wizard.ui")]
    pub struct PifWizard {
        #[template_child]
        pub carousel: TemplateChild<adw::Carousel>,
        #[template_child]
        pub dots: TemplateChild<adw::CarouselIndicatorDots>,
        #[template_child]
        pub step_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub status_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub back_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub next_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub editor_button: TemplateChild<gtk::Button>,

        // Step 1
        #[template_child]
        pub name_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub app_id_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub app_id_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub app_id_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub summary_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub summary_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub summary_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub description_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub license_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub license_value: TemplateChild<gtk::Label>,
        #[template_child]
        pub license_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub developer_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub homepage_entry: TemplateChild<gtk::Entry>,

        // Step 2
        #[template_child]
        pub runtime_combo: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub runtime_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub runtime_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub copy_install_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub extensions_list: TemplateChild<gtk::ListBox>,

        // Step 3
        #[template_child]
        pub sources_list: TemplateChild<gtk::ListBox>,

        // Step 4
        #[template_child]
        pub buildsystem_combo: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub buildsystem_status: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub command_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub commands_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub commands_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub options_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub options_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub env_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub module_preview: TemplateChild<gtk::TextView>,

        // Steps 5, 6 and 7, all filled by `panes`
        #[template_child]
        pub dependencies_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub permissions_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub appearance_box: TemplateChild<gtk::Box>,

        // Step 7
        #[template_child]
        pub files_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub files_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub target_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub build_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub backup_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub issues_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub issues_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub full_preview: TemplateChild<gtk::TextView>,

        /// Shared with the window and the editor; never a copy.
        pub handle: RefCell<Option<Handle>>,
        pub runtime_choices: RefCell<Vec<runtimes::Choice>>,
        /// Files the user unticked on the review step. Kept here rather than in
        /// the plan, because the plan is worked out afresh on every redraw.
        pub skipped: RefCell<Vec<generate::FileKind>>,
        /// Set when the user asks for the editor, so the window knows which mode
        /// to open once this page has gone.
        pub wants_editor: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PifWizard {
        const NAME: &'static str = "PifWizard";
        type Type = super::PifWizard;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PifWizard {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PifWizard {}
    impl NavigationPageImpl for PifWizard {}
}

glib::wrapper! {
    pub struct PifWizard(ObjectSubclass<imp::PifWizard>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PifWizard {
    pub fn new(project: Rc<RefCell<Project>>) -> Self {
        // Opening a project to work on it is the moment to make sure the build
        // can find the compilers it declares. An import is left alone until
        // then, but nothing should reach the build page missing this.
        runtimes::sync_build_paths(&mut project.borrow_mut().manifest);

        let wizard: Self = glib::Object::builder().build();

        let handle = Handle::new(
            project,
            clone!(
                #[weak(rename_to = wizard)]
                wizard,
                move || wizard.refresh()
            ),
        )
        .autosave();
        wizard.imp().handle.replace(Some(handle));

        wizard.load();
        wizard.refresh_runtimes_in_background();
        wizard
    }

    fn handle(&self) -> Handle {
        self.imp()
            .handle
            .borrow()
            .clone()
            .expect("the wizard is only built with a project")
    }

    /// Whether the user left through the "Editor" button rather than going back.
    pub fn wanted_editor(&self) -> bool {
        self.imp().wants_editor.get()
    }

    // -- wiring --------------------------------------------------------------

    fn setup(&self) {
        let imp = self.imp();

        let actions = gio::SimpleActionGroup::new();
        for (name, kind) in [
            ("add-folder", SourceKind::Dir),
            ("add-git", SourceKind::Git),
            ("add-archive", SourceKind::Archive),
            ("add-file", SourceKind::File),
        ] {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate(clone!(
                #[weak(rename_to = wizard)]
                self,
                move |_, _| {
                    forms::add_source(&wizard.handle(), kind.clone(), &wizard.rebuild_sources());
                }
            ));
            actions.add_action(&action);
        }
        self.insert_action_group("wizard", Some(&actions));

        for entry in [
            &*imp.name_entry,
            &imp.app_id_entry,
            &imp.summary_entry,
            &imp.developer_entry,
            &imp.homepage_entry,
            &imp.command_entry,
        ] {
            entry.connect_changed(clone!(
                #[weak(rename_to = wizard)]
                self,
                move |_| wizard.collect()
            ));
        }

        for view in [
            &*imp.description_view,
            &imp.commands_view,
            &imp.options_view,
            &imp.env_view,
        ] {
            view.buffer().connect_changed(clone!(
                #[weak(rename_to = wizard)]
                self,
                move |_| wizard.collect()
            ));
        }

        imp.license_row.connect_activated(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |row| {
                let handle = wizard.handle();
                let current = handle.read(|project| project.license.clone());
                forms::choose_licence(row, &current, move |id| {
                    handle.write(|project| project.license = id.to_string());
                });
            }
        ));

        for combo in [&*imp.runtime_combo, &imp.buildsystem_combo] {
            combo.connect_selected_notify(clone!(
                #[weak(rename_to = wizard)]
                self,
                move |_| wizard.collect()
            ));
        }

        // The dots measure themselves from the carousel's snap points, and the
        // carousel has none until it has been allocated once. So the bottom bar
        // is laid out while they still claim the width of a single dot, and they
        // then draw eight of them straight over the status text beside them.
        // Measured with PACKITFLAT_DEV_GEOMETRY: allocated 27px, drawing ~130.
        imp.dots.connect_map(|dots| {
            let dots = dots.clone();
            glib::idle_add_local_once(move || dots.queue_resize());
        });

        imp.carousel.connect_page_changed(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_, _| {
                wizard.refresh();
                wizard.save();
            }
        ));

        imp.back_button.connect_clicked(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_| wizard.step_by(-1)
        ));
        imp.next_button.connect_clicked(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_| {
                if wizard.current_step() + 1 == STEPS.len() as u32 {
                    wizard.generate();
                } else {
                    wizard.step_by(1);
                }
            }
        ));

        // Switching modes is a pop: the window sees which button was pressed and
        // pushes the other mode over the same project.
        imp.editor_button.connect_clicked(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_| {
                wizard.imp().wants_editor.set(true);
                wizard.save();
                if let Some(view) = wizard.parent().and_downcast::<adw::NavigationView>() {
                    view.pop();
                }
            }
        ));

        imp.copy_install_button.connect_clicked(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |button| {
                if let Some(choice) = wizard.selected_runtime() {
                    wizard.clipboard().set_text(&choice.install_command());
                    button.set_label(&t("Copied"));
                    let button = button.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(2), move || {
                        button.set_label(&t("Copy command"));
                    });
                }
            }
        ));

        imp.backup_row.connect_active_notify(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_| wizard.refresh()
        ));

        imp.build_row.connect_activated(clone!(
            #[weak(rename_to = wizard)]
            self,
            move |_| crate::build_page::PifBuild::push_from(&wizard, wizard.handle().shared())
        ));
    }

    /// Rebuilding the source list is needed whenever its shape changes, because
    /// the row callbacks capture positions.
    fn rebuild_sources(&self) -> Rc<dyn Fn()> {
        let wizard = self.downgrade();
        Rc::new(move || {
            if let Some(wizard) = wizard.upgrade() {
                wizard.load_sources();
                wizard.refresh();
            }
        })
    }

    // -- filling the widgets from the project --------------------------------

    fn load(&self) {
        let imp = self.imp();
        let handle = self.handle();

        handle.silently(|| {
            handle.read(|project| {
                imp.name_entry.set_text(&project.name);
                imp.app_id_entry.set_text(&project.manifest.app_id);
                imp.summary_entry.set_text(&project.summary);
                imp.developer_entry.set_text(&project.developer);
                imp.homepage_entry.set_text(&project.homepage);
                imp.description_view.buffer().set_text(&project.description);
                imp.command_entry.set_text(&project.manifest.command);

                let build_names: Vec<String> = forms::BUILD_SYSTEMS
                    .iter()
                    .map(|(_, label, _)| t(label))
                    .collect();
                imp.buildsystem_combo
                    .set_model(Some(&string_list(&build_names)));

                if let Some(module) = project.manifest.main_module() {
                    imp.buildsystem_combo
                        .set_selected(forms::build_system_index(module.buildsystem.as_ref()));
                    forms::set_lines(&*imp.commands_view, &module.build_commands);
                    forms::set_lines(&*imp.options_view, &module.config_opts);
                    let env = module
                        .build_options
                        .as_ref()
                        .map(|options| forms::env_lines(&options.env))
                        .unwrap_or_default();
                    forms::set_lines(&*imp.env_view, &env);
                }
            });
        });

        self.load_runtimes();
        self.load_extensions();
        self.load_sources();
        self.load_panes();
        self.refresh();
        forms::hook_expanders(self);
    }

    /// Steps 5, 6 and 7 are built by `panes`, which the editor uses too.
    fn load_panes(&self) {
        let imp = self.imp();
        let handle = self.handle();
        let rebuild = self.rebuild_panes();

        panes::install_permission_actions(self, &handle, rebuild.clone());
        panes::build_dependencies(&imp.dependencies_box, &handle, rebuild.clone());
        panes::build_permissions(&imp.permissions_box, &handle, rebuild.clone());
        panes::build_appearance(&imp.appearance_box, &handle, rebuild);
    }

    /// The permission and appearance panes rebuild wholesale: their rows depend
    /// on what is in the lists, so a change of shape means a fresh set of rows.
    fn rebuild_panes(&self) -> Rc<dyn Fn()> {
        let wizard = self.downgrade();
        Rc::new(move || {
            if let Some(wizard) = wizard.upgrade() {
                let handle = wizard.handle();
                let imp = wizard.imp();
                let again = wizard.rebuild_panes();
                panes::build_dependencies(&imp.dependencies_box, &handle, again.clone());
                panes::build_permissions(&imp.permissions_box, &handle, again.clone());
                panes::build_appearance(&imp.appearance_box, &handle, again);
                wizard.refresh();
            }
        })
    }

    /// The runtime picker, from the cached flathub listing plus the bundled
    /// fallback. Called again when a fresh listing arrives.
    fn load_runtimes(&self) {
        let imp = self.imp();
        let handle = self.handle();
        let (current_runtime, current_version) = handle.read(|project| {
            (
                project.manifest.runtime.clone(),
                project.manifest.runtime_version.clone(),
            )
        });

        let available = runtimes::load_cached()
            .map(|text| runtimes::parse_list(&text))
            .unwrap_or_default();
        let installed = runtimes::load_cached_installed()
            .map(|text| runtimes::parse_list(&text))
            .unwrap_or_default();
        let mut choices = runtimes::catalogue(&available, &installed);

        // Whatever the manifest already says has to be offered, even if flathub
        // has never heard of it — otherwise opening someone's manifest silently
        // changes their runtime.
        if !current_runtime.is_empty()
            && runtimes::position_of(&choices, &current_runtime, &current_version).is_none()
        {
            choices.insert(
                0,
                runtimes::Choice {
                    sdk: runtimes::sdk_for(&current_runtime),
                    friendly: runtimes::friendly(&current_runtime, &current_version),
                    support: runtimes::support(&current_runtime, &current_version),
                    installed: installed
                        .contains(&(current_runtime.clone(), current_version.clone())),
                    runtime: current_runtime.clone(),
                    version: current_version.clone(),
                },
            );
        }

        let names: Vec<String> = choices.iter().map(|c| c.friendly.clone()).collect();
        let selected =
            runtimes::position_of(&choices, &current_runtime, &current_version).unwrap_or(0) as u32;

        handle.silently(|| {
            imp.runtime_combo.set_model(Some(&string_list(&names)));
            imp.runtime_combo.set_selected(selected);
        });
        imp.runtime_choices.replace(choices);
    }

    fn load_extensions(&self) {
        forms::fill_extensions(&self.imp().extensions_list, &self.handle());
    }

    fn load_sources(&self) {
        forms::fill_sources(
            &self.imp().sources_list,
            &self.handle(),
            self,
            self.rebuild_sources(),
        );
    }

    // -- reading the widgets back into the project ---------------------------

    fn collect(&self) {
        let imp = self.imp();
        let handle = self.handle();
        if handle.is_muted() {
            return;
        }

        handle.write(|project| {
            project.name = imp.name_entry.text().to_string();
            project.manifest.app_id = imp.app_id_entry.text().trim().to_string();
            project.summary = imp.summary_entry.text().to_string();
            project.developer = imp.developer_entry.text().to_string();
            project.homepage = imp.homepage_entry.text().to_string();
            project.description = forms::text_of(&*imp.description_view);
            project.manifest.command = imp.command_entry.text().trim().to_string();

            // The licence is not read back from a widget: the picker writes it
            // when one is chosen, and there is nothing here to collect.

            if let Some(choice) = imp
                .runtime_choices
                .borrow()
                .get(imp.runtime_combo.selected() as usize)
            {
                project.manifest.runtime = choice.runtime.clone();
                project.manifest.runtime_version = choice.version.clone();
                project.manifest.sdk = choice.sdk.clone();
            }

            let name = module_name(project);
            let build = forms::build_system_at(imp.buildsystem_combo.selected());
            let commands = forms::lines_of(&*imp.commands_view);
            let options = forms::lines_of(&*imp.options_view);
            let env = forms::env_mapping(&forms::lines_of(&*imp.env_view));

            let module = project.manifest.ensure_main_module(&name);
            module.buildsystem = build;
            module.build_commands = commands;
            module.config_opts = options;
            forms::set_module_env(module, env);

            // An icon already sitting where this app puts them belongs to this
            // project. Looked for on every edit, not only when the project was
            // opened: the icon is named after the app ID, so a project started
            // from a folder has nothing to find until the ID is typed.
            packitflat::icons::adopt_existing(project);
            // A hand-written build installs only what its commands say, so the
            // desktop entry, the icon and the metainfo have to be in there or
            // the finished app is a binary and nothing else.
            generate::sync_install_commands(project);
        });
    }

    // -- everything derived from the project ---------------------------------

    fn refresh(&self) {
        let imp = self.imp();
        let handle = self.handle();
        let issues = handle.read(validate::project);
        let step = self.current_step();

        imp.step_title.set_title(&t(STEPS[step as usize]));
        imp.step_title
            .set_subtitle(&format!("Step {} of {}", step + 1, STEPS.len()));

        imp.back_button.set_sensitive(step > 0);
        let last = step + 1 == STEPS.len() as u32;
        imp.next_button
            .set_label(&if last { t("Write the manifest") } else { t("Next") });
        imp.next_button.set_sensitive(!last || issues.errors() == 0);
        imp.status_label.set_label(&status_text(&issues, step));

        // Step 1: inline feedback under the field it belongs to.
        show_issue(
            &imp.app_id_status,
            &imp.app_id_icon,
            first_for(&issues, validate::Field::AppId),
        );
        show_issue(
            &imp.summary_status,
            &imp.summary_icon,
            first_for(&issues, validate::Field::Summary),
        );

        imp.license_value
            .set_label(&handle.read(|project| forms::licence_summary(&project.license)));
        let license = handle.read(|project| spdx::find(project.license.trim()));
        imp.license_status.set_title(&match license {
            Some(license) => t(license.name),
            None => t("No licence chosen"),
        });
        imp.license_status.set_subtitle(&match license {
            Some(license) => t(license.summary),
            None => t(
                "Without one, nobody knows what they may do with your code, and app \
                 stores will not list it.",
            ),
        });

        // Step 2: what the chosen runtime means.
        if let Some(choice) = self.selected_runtime() {
            imp.runtime_status.set_title(&if choice.installed {
                t("Already on this computer")
            } else {
                t("Not installed yet")
            });
            imp.runtime_status.set_subtitle(&format!(
                "{} {}",
                t(choice.support.explanation()),
                if choice.installed {
                    String::new()
                } else {
                    t("It will have to be installed before the app can be built.")
                }
            ));
            imp.copy_install_button.set_visible(!choice.installed);

            let severity = match choice.support {
                runtimes::Support::Current => None,
                runtimes::Support::Ageing | runtimes::Support::Unknown => Some(Severity::Warning),
                runtimes::Support::EndOfLife => Some(Severity::Error),
            };
            imp.runtime_icon.set_icon_name(Some(match severity {
                None if choice.installed => "object-select-symbolic",
                None => "folder-download-symbolic",
                Some(Severity::Warning) => "dialog-information-symbolic",
                Some(Severity::Error) => "dialog-warning-symbolic",
            }));
            forms::set_note_style(&imp.runtime_status, severity);
        }

        // Step 4: only one of the two option boxes applies at a time.
        let simple = matches!(
            forms::build_system_at(imp.buildsystem_combo.selected()),
            None | Some(BuildSystem::Simple)
        );
        imp.commands_group.set_visible(simple);
        imp.options_group.set_visible(!simple);
        imp.buildsystem_status
            .set_title(&forms::build_system_explanation(
                imp.buildsystem_combo.selected(),
            ));
        imp.module_preview
            .buffer()
            .set_text(&handle.read(|project| forms::module_yaml(&project.manifest)));

        // Step 7: every file that would be written, with what it is for.
        self.fill_files();
        imp.full_preview
            .buffer()
            .set_text(&handle.read(packitflat::sync::text_for));

        forms::clear(&imp.issues_list);
        for issue in &issues {
            let target = issue.field.step();
            let go_there: Option<Box<dyn Fn()>> = (target != step).then(|| {
                let wizard = self.downgrade();
                Box::new(move || {
                    if let Some(wizard) = wizard.upgrade() {
                        wizard.go_to_step(target);
                    }
                }) as Box<dyn Fn()>
            });
            imp.issues_list.append(&forms::issue_row(issue, go_there));
        }
        imp.issues_group.set_visible(!issues.is_empty());
    }

    /// The review step's file list: one row per file, each saying what it is for
    /// and each switchable. Nothing appears on disk that wasn't described here
    /// first.
    fn fill_files(&self) {
        let imp = self.imp();
        forms::clear(&imp.files_list);

        let plan = match self.current_plan() {
            Ok(plan) => plan,
            Err(err) => {
                imp.target_row.set_subtitle(&err.friendly());
                imp.backup_row.set_visible(false);
                imp.files_group.set_visible(false);
                forms::set_note_style(&imp.target_row, Some(Severity::Error));
                return;
            }
        };

        imp.files_group.set_visible(true);
        imp.target_row.set_subtitle(&plan.folder.display().to_string());
        forms::set_note_style(&imp.target_row, None);
        imp.backup_row.set_visible(plan.replaces_anything());

        for file in &plan.files {
            let kind = file.kind;
            // The path is only worth repeating when the file goes somewhere
            // other than the folder itself — the icon, for instance.
            let mut subtitle = if file.relative.parent().is_some_and(|parent| parent.as_os_str().is_empty()) {
                t(kind.purpose())
            } else {
                format!("{}\n{}", file.relative.display(), t(kind.purpose()))
            };
            if file.replaces_existing {
                subtitle.push('\n');
                subtitle.push_str(&t("A file of this name is already there."));
            }
            if let Some(note) = &file.note {
                subtitle.push('\n');
                subtitle.push_str(note);
            }

            let row = adw::SwitchRow::builder()
                .title(file.file_name())
                .subtitle(subtitle)
                .subtitle_lines(0)
                .active(file.include)
                .build();
            if file.note.is_some() {
                row.add_css_class("note-info");
            }
            row.connect_active_notify(clone!(
                #[weak(rename_to = wizard)]
                self,
                move |row| {
                    let mut skipped = wizard.imp().skipped.borrow_mut();
                    skipped.retain(|other| *other != kind);
                    if !row.is_active() {
                        skipped.push(kind);
                    }
                    drop(skipped);
                    // The install line goes with the file: one that won't be
                    // written must not be installed, and one switched back on
                    // needs its line again.
                    if let Ok(plan) = wizard.current_plan() {
                        wizard.handle().write(|project| {
                            generate::sync_install_commands_with(project, &plan);
                        });
                    }
                    wizard.refresh();
                }
            ));
            imp.files_list.append(&row);
        }
    }

    /// The plan as it stands, with the review step's switches applied.
    fn current_plan(&self) -> Result<generate::Plan, generate::GenerateError> {
        let mut plan = self.handle().read(generate::plan)?;
        for kind in self.imp().skipped.borrow().iter() {
            plan.set_included(*kind, false);
        }
        Ok(plan)
    }

    fn selected_runtime(&self) -> Option<runtimes::Choice> {
        let imp = self.imp();
        let index = imp.runtime_combo.selected() as usize;
        imp.runtime_choices.borrow().get(index).cloned()
    }

    // -- navigation ----------------------------------------------------------

    fn current_step(&self) -> u32 {
        self.imp().carousel.position().round().max(0.0) as u32
    }

    fn step_by(&self, delta: i32) {
        let step = self.current_step() as i32 + delta;
        self.go_to_step(step.clamp(0, STEPS.len() as i32 - 1) as u32);
    }

    /// Type into the first step the way a person would, for the harness.
    #[cfg(debug_assertions)]
    pub fn dev_fill_basics(&self) {
        let imp = self.imp();
        imp.name_entry.set_text("Typed Name");
        imp.app_id_entry.set_text("no.oyzmo.Typed");
        imp.developer_entry.set_text("Typed Developer");
        imp.homepage_entry.set_text("http://typed.example");
        imp.summary_entry.set_text("Typed summary");
    }

    pub fn go_to_step(&self, step: u32) {
        let carousel = &self.imp().carousel;
        if step < carousel.n_pages() {
            carousel.scroll_to(&carousel.nth_page(step), true);
        }
    }

    // -- writing the manifest ------------------------------------------------

    fn generate(&self) {
        match self.current_plan() {
            Ok(plan) => {
                let backup = self.imp().backup_row.is_active();
                forms::write_files(&self.handle(), self, &plan, backup);
                self.save();
                self.refresh();
            }
            Err(err) => self.show_error(&t("The files can't be written yet"), &err.friendly()),
        }
    }

    // -- odds and ends -------------------------------------------------------

    /// Ask flatpak what exists, in the background, and rebuild the picker if the
    /// answer differs from the cache. Failure is silent on purpose: the bundled
    /// list already works, and "flatpak isn't installed" is not something to
    /// interrupt someone with while they're filling in a form.
    fn refresh_runtimes_in_background(&self) {
        glib::spawn_future_local(clone!(
            #[weak(rename_to = wizard)]
            self,
            async move {
                let mut changed = false;
                if runtimes::cache_is_stale() {
                    if let Some(text) = run_flatpak(runtimes::REMOTE_LS_ARGS).await {
                        let _ = runtimes::store_cached(&text);
                        changed = true;
                    }
                }
                if let Some(text) = run_flatpak(runtimes::LIST_INSTALLED_ARGS).await {
                    let _ = runtimes::store_cached_installed(&text);
                    changed = true;
                }
                if changed {
                    wizard.load_runtimes();
                    wizard.refresh();
                }
            }
        ));
    }

    /// Save at once, wherever waiting would be wrong: the page is going away.
    fn save(&self) {
        self.handle().save_now();
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

    /// Open the licence picker, type into its search box, choose the first thing
    /// that comes back, and report what the project was left holding.
    ///
    /// `spdx::search` is unit-tested; what a unit test cannot reach is whether
    /// the widgets are using it, and that is the part that was broken — twice
    /// over. The last check is the important one: a picker that narrows its list
    /// correctly and then stores a *different* licence is worse than one that
    /// visibly does nothing.
    #[cfg(debug_assertions)]
    pub fn dev_licence_check(&self, query: &str) {
        self.go_to_step(0);
        adw::prelude::ActionRowExt::activate(&*self.imp().license_row);

        let query = query.to_string();
        let wizard = self.clone();
        let mut waited = 0;
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            let Some(root) = wizard.root().map(|root| root.upcast::<gtk::Widget>()) else {
                return glib::ControlFlow::Continue;
            };
            let dialog = forms::descendant::<adw::Dialog>(&root);
            let entry = dialog
                .as_ref()
                .and_then(|dialog| forms::descendant::<gtk::SearchEntry>(dialog.upcast_ref()));
            if entry.is_none() && waited < 100 {
                waited += 1;
                return glib::ControlFlow::Continue;
            }

            let mut ok = true;
            let mut check = |what: &str, passed: bool| {
                eprintln!("licence-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                ok &= passed;
            };
            check("the licence row opens a picker", dialog.is_some());
            check("the picker has a search box", entry.is_some());
            let (Some(dialog), Some(entry)) = (dialog, entry) else {
                std::process::exit(1);
            };

            let Some(list) =
                forms::descendant_named::<gtk::ListBox>(dialog.upcast_ref(), "licence-list")
            else {
                eprintln!("licence-check: FAIL the picker has no list");
                std::process::exit(1);
            };
            let titles = |list: &gtk::ListBox| {
                let mut titles = Vec::new();
                let mut child = list.first_child();
                while let Some(widget) = child {
                    if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                        titles.push(row.title().to_string());
                    }
                    child = widget.next_sibling();
                }
                titles
            };
            let before = titles(&list).len();
            check("the whole list is offered before typing", before > 1);
            if !ok {
                std::process::exit(1);
            }

            entry.set_text(&query);
            // A GtkSearchEntry holds a typed query back for a moment before it
            // tells anyone, so reading the list straight away reads the old one.
            let mut waited = 0;
            let query = query.clone();
            let wizard = wizard.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let now = titles(&list);
                if now.len() == before && waited < 60 {
                    waited += 1;
                    return glib::ControlFlow::Continue;
                }

                let mut ok = true;
                let mut check = |what: &str, passed: bool| {
                    eprintln!("licence-check: {} {what}", if passed { "ok  " } else { "FAIL" });
                    ok &= passed;
                };

                eprintln!(
                    "licence-check:      “{query}” leaves {} of {before}",
                    now.len()
                );
                check("typing leaves something", !now.is_empty());
                check("typing narrows the list", now.len() < before);

                let first = now.first().cloned().unwrap_or_default();
                eprintln!("licence-check:      first match: {first}");
                check(
                    "what came back is what was asked for",
                    spdx::searchable(&first).contains(&query.to_lowercase()),
                );

                // And then the half that used to hand back the wrong licence.
                let wanted = first.split(" — ").next().unwrap_or_default().to_string();
                let mut child = list.first_child();
                while let Some(widget) = child {
                    if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                        adw::prelude::ActionRowExt::activate(row);
                        break;
                    }
                    child = widget.next_sibling();
                }
                let stored = wizard.handle().read(|project| project.license.clone());
                eprintln!("licence-check:      choosing it stored “{stored}”");
                check("choosing a licence stores that licence", stored == wanted);

                std::process::exit(if ok { 0 } else { 1 });
            });

            glib::ControlFlow::Break
        });
    }
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

/// `flatpak` on the host. Inside the sandbox that means going through
/// flatpak-spawn; outside it, running it directly. `None` on any failure — every
/// caller has a working fallback.
pub async fn run_flatpak(args: &[&str]) -> Option<String> {
    let sandboxed = Path::new("/.flatpak-info").exists();
    let mut argv: Vec<&str> = if sandboxed {
        vec!["flatpak-spawn", "--host", "flatpak"]
    } else {
        vec!["flatpak"]
    };
    argv.extend_from_slice(args);

    let process = gio::Subprocess::newv(
        &argv.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>(),
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .ok()?;

    let (stdout, _) = process.communicate_utf8_future(None).await.ok()?;
    let text = stdout?.to_string();
    (!text.trim().is_empty()).then_some(text)
}

fn status_text(issues: &[validate::Issue], step: u32) -> String {
    let here = issues.for_step(step);
    let blocking = here.iter().filter(|i| i.severity == Severity::Error).count();

    if blocking > 0 {
        return if blocking == 1 {
            t("One thing on this page still needs an answer")
        } else {
            format!("{blocking} things on this page still need answers")
        };
    }
    if !here.is_empty() {
        return if here.len() == 1 {
            t("One suggestion on this page")
        } else {
            format!("{} suggestions on this page", here.len())
        };
    }

    let total = issues.errors();
    if total == 0 {
        t("Everything needed is filled in")
    } else if total == 1 {
        t("One thing left, on another page")
    } else {
        format!("{total} things left, on other pages")
    }
}

fn first_for(issues: &[validate::Issue], field: validate::Field) -> Option<&validate::Issue> {
    issues.iter().find(|issue| issue.field == field)
}

fn show_issue(row: &adw::ActionRow, icon: &gtk::Image, issue: Option<&validate::Issue>) {
    match issue {
        Some(issue) => {
            row.set_title(&issue.message);
            row.set_subtitle(&issue.fix);
            row.set_visible(true);
            icon.set_icon_name(Some(match issue.severity {
                Severity::Error => "dialog-warning-symbolic",
                Severity::Warning => "dialog-information-symbolic",
            }));
            forms::set_note_style(row, Some(issue.severity));
        }
        None => row.set_visible(false),
    }
}

fn string_list(items: &[String]) -> gtk::StringList {
    let list = gtk::StringList::new(&[]);
    for item in items {
        list.append(item);
    }
    list
}
