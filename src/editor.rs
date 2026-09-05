//! Editor mode: a sidebar of the manifest's parts, a form for each, and the file
//! itself as text.
//!
//! It edits the same project as the guided steps, through the same
//! [`forms::Handle`], with the same validation. The one thing only this mode has
//! is the raw YAML pane, and the rule there is the brief's: hand-edits that
//! can't be parsed are reported, never overwritten. That policy is
//! `packitflat::sync`, tested away from the widgets; this file only wires it up.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib::clone;
use gtk::{gio, glib};

use packitflat::i18n::t;
use packitflat::manifest::{BuildSystem, ModuleEntry, SourceKind};
use packitflat::project::Project;
use packitflat::validate::{self, Issues};
use packitflat::{runtimes, sync};

use crate::forms::{self, Handle};
use crate::panes;
use sourceview::prelude::*;

/// One editable line of the manifest pane: label, explanation, example, current
/// value, and where the typed value goes.
type Field = (String, String, &'static str, String, fn(&mut Project, String));

/// What the sidebar offers, in order. The last variant is a module the app keeps
/// but doesn't model — an imported manifest's shared-modules, say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Manifest,
    Sources,
    Build,
    Dependencies,
    Permissions,
    Appearance,
    Yaml,
    Problems,
    OtherModule(String),
}

impl Part {
    fn pane(&self) -> &'static str {
        match self {
            Part::Manifest => "manifest",
            Part::Sources => "sources",
            Part::Build => "build",
            Part::Dependencies => "dependencies",
            Part::Permissions => "permissions",
            Part::Appearance => "appearance",
            Part::Yaml => "yaml",
            Part::Problems => "problems",
            Part::OtherModule(_) => "other-module",
        }
    }

    fn title(&self) -> String {
        match self {
            Part::Manifest => t("The manifest"),
            Part::Sources => t("Where the code comes from"),
            Part::Build => t("How it's built"),
            Part::Dependencies => t("What it downloads"),
            Part::Permissions => t("What it may do"),
            Part::Appearance => t("How it looks"),
            Part::Yaml => t("The manifest text"),
            Part::Problems => t("Problems"),
            Part::OtherModule(name) => name.clone(),
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Part::Manifest => "document-properties-symbolic",
            Part::Sources => "folder-symbolic",
            Part::Build => "system-run-symbolic",
            Part::Dependencies => "folder-download-symbolic",
            Part::Permissions => "security-medium-symbolic",
            Part::Appearance => "applications-graphics-symbolic",
            Part::Yaml => "text-x-generic-symbolic",
            Part::Problems => "dialog-warning-symbolic",
            Part::OtherModule(_) => "package-x-generic-symbolic",
        }
    }
}

mod imp {
    use super::*;
    use std::cell::Cell;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/no/oyzmo/PackItFlat/ui/editor.ui")]
    pub struct PifEditor {
        #[template_child]
        pub split: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub detail_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub tree: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub panes: TemplateChild<gtk::Stack>,
        #[template_child]
        pub guided_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub write_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub build_button: TemplateChild<gtk::Button>,

        // Manifest pane
        #[template_child]
        pub about_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub runtime_combo: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub sdk_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub extensions_list: TemplateChild<gtk::ListBox>,

        // Sources pane
        #[template_child]
        pub sources_list: TemplateChild<gtk::ListBox>,

        // Build pane
        #[template_child]
        pub build_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub buildsystem_combo: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub buildsystem_status: TemplateChild<adw::ActionRow>,
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

        // Filled by `panes`, the same as the guided steps' last two pages
        #[template_child]
        pub dependencies_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub permissions_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub appearance_box: TemplateChild<gtk::Box>,

        // Raw YAML pane. A GtkSourceView *is* a GtkTextView, so everything the
        // sync wiring does with the buffer is unchanged by the swap.
        #[template_child]
        pub yaml_view: TemplateChild<sourceview::View>,
        #[template_child]
        pub yaml_banner: TemplateChild<adw::Banner>,

        // Problems pane
        #[template_child]
        pub problems_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub problems_list: TemplateChild<gtk::ListBox>,

        #[template_child]
        pub other_module_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub other_module_button: TemplateChild<gtk::Button>,

        pub handle: RefCell<Option<Handle>>,
        /// The rows the manifest pane built, so they can be taken out again.
        pub about_rows: RefCell<Vec<adw::ActionRow>>,
        pub parts: RefCell<Vec<Part>>,
        pub runtime_choices: RefCell<Vec<runtimes::Choice>>,
        /// Where the caret should go for the current parse error, if anywhere.
        pub error_position: Cell<Option<(usize, usize)>>,
        /// Set when the user asks for the guided steps instead.
        pub wants_guided: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PifEditor {
        const NAME: &'static str = "PifEditor";
        type Type = super::PifEditor;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PifEditor {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PifEditor {}
    impl NavigationPageImpl for PifEditor {}
}

glib::wrapper! {
    pub struct PifEditor(ObjectSubclass<imp::PifEditor>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PifEditor {
    pub fn new(project: Rc<RefCell<Project>>) -> Self {
        // As in the wizard: working on a project means its declared compilers
        // have to be findable by the build.
        runtimes::sync_build_paths(&mut project.borrow_mut().manifest);

        let editor: Self = glib::Object::builder().build();

        let handle = Handle::new(
            project,
            clone!(
                #[weak(rename_to = editor)]
                editor,
                move || editor.refresh()
            ),
        )
        .autosave();
        editor.imp().handle.replace(Some(handle));
        editor.load();
        editor
    }

    fn handle(&self) -> Handle {
        self.imp()
            .handle
            .borrow()
            .clone()
            .expect("the editor is only built with a project")
    }

    /// Whether the user left through the "Guided steps" button.
    pub fn wanted_guided(&self) -> bool {
        self.imp().wants_guided.get()
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
                #[weak(rename_to = editor)]
                self,
                move |_, _| {
                    forms::add_source(&editor.handle(), kind.clone(), &editor.rebuild_sources())
                }
            ));
            actions.add_action(&action);
        }
        self.insert_action_group("editor", Some(&actions));

        imp.tree.connect_row_selected(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_, row| {
                let Some(row) = row else { return };
                let index = row.index().max(0) as usize;
                let part = editor.imp().parts.borrow().get(index).cloned();
                if let Some(part) = part {
                    editor.show_part(&part);
                }
            }
        ));

        imp.guided_button.connect_clicked(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| {
                editor.imp().wants_guided.set(true);
                editor.save();
                if let Some(view) = editor.parent().and_downcast::<adw::NavigationView>() {
                    view.pop();
                }
            }
        ));

        imp.write_button.connect_clicked(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| forms::write_manifest(&editor.handle(), &editor, true)
        ));
        imp.build_button.connect_clicked(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| crate::build_page::PifBuild::push_from(&editor, editor.handle().shared())
        ));

        imp.other_module_button.connect_clicked(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| editor.show_part(&Part::Yaml)
        ));

        for combo in [&*imp.runtime_combo, &imp.buildsystem_combo] {
            combo.connect_selected_notify(clone!(
                #[weak(rename_to = editor)]
                self,
                move |_| editor.collect()
            ));
        }

        for view in [&*imp.commands_view, &imp.options_view, &imp.env_view] {
            view.buffer().connect_changed(clone!(
                #[weak(rename_to = editor)]
                self,
                move |_| editor.collect()
            ));
        }

        self.setup_highlighting();

        // The raw YAML pane: what is typed goes into the model when it parses,
        // and nothing at all happens when it doesn't.
        imp.yaml_view.buffer().connect_changed(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| editor.take_yaml()
        ));

        imp.yaml_banner.connect_button_clicked(clone!(
            #[weak(rename_to = editor)]
            self,
            move |_| editor.jump_to_error()
        ));
    }

    /// YAML highlighting, and a colour scheme that follows the rest of the app.
    /// Without the scheme the text keeps GtkSourceView's own light colours on a
    /// dark window, which looks like a bug.
    fn setup_highlighting(&self) {
        let Ok(buffer) = self
            .imp()
            .yaml_view
            .buffer()
            .downcast::<sourceview::Buffer>()
        else {
            return;
        };

        if let Some(language) = sourceview::LanguageManager::default().language("yaml") {
            buffer.set_language(Some(&language));
        }
        buffer.set_highlight_syntax(true);
        buffer.set_highlight_matching_brackets(true);

        let apply = move |dark: bool| {
            let name = if dark { "Adwaita-dark" } else { "Adwaita" };
            if let Some(scheme) = sourceview::StyleSchemeManager::default().scheme(name) {
                buffer.set_style_scheme(Some(&scheme));
            }
        };

        let manager = adw::StyleManager::default();
        apply(manager.is_dark());
        manager.connect_dark_notify(move |manager| apply(manager.is_dark()));
    }

    fn rebuild_sources(&self) -> Rc<dyn Fn()> {
        let editor = self.downgrade();
        Rc::new(move || {
            if let Some(editor) = editor.upgrade() {
                editor.load_sources();
                editor.refresh();
            }
        })
    }

    // -- filling in ----------------------------------------------------------

    fn load(&self) {
        self.load_tree();
        self.load_manifest_pane();
        self.load_runtimes();
        self.load_extensions();
        self.load_sources();
        self.load_build_pane();
        self.load_panes();
        self.load_yaml();
        self.refresh();
        self.show_part(&Part::Manifest);
        forms::hook_expanders(self);
    }

    /// The sidebar. Rebuilt when the manifest's shape changes, because it lists
    /// whatever modules the manifest actually has.
    fn load_tree(&self) {
        let imp = self.imp();
        forms::clear(&imp.tree);

        let mut parts = vec![Part::Manifest, Part::Sources, Part::Build];
        let others = self.handle().read(|project| {
            let main = project.manifest.main_module().map(|module| module.name.clone());
            project
                .manifest
                .modules
                .iter()
                .filter_map(|entry| match entry {
                    ModuleEntry::Module(module) if Some(&module.name) != main.as_ref() => {
                        Some(module.name.clone())
                    }
                    ModuleEntry::Include(path) => Some(path.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        parts.extend(others.into_iter().map(Part::OtherModule));
        parts.push(Part::Dependencies);
        parts.push(Part::Permissions);
        parts.push(Part::Appearance);
        parts.push(Part::Yaml);
        parts.push(Part::Problems);

        for part in &parts {
            let row = adw::ActionRow::builder().title(part.title()).build();
            row.add_prefix(&gtk::Image::from_icon_name(part.icon()));
            imp.tree.append(&row);
        }
        imp.parts.replace(parts);
    }

    /// The manifest pane's text fields, built from the shared row builder so they
    /// carry the same labels and explanations as the guided steps.
    fn load_manifest_pane(&self) {
        let imp = self.imp();
        let handle = self.handle();

        // The rows this built last time, removed by the handles kept for them.
        //
        // Walking the group's children instead does not work: an
        // AdwPreferencesGroup's first child is its own internal box, never a
        // row, so the obvious loop stops immediately and every rebuild *appends*
        // another set of fields. That left the stale first set — the one built
        // before anything had been typed — sitting at the top of the pane
        // looking like the app had forgotten what was entered.
        for row in imp.about_rows.borrow_mut().drain(..) {
            imp.about_group.remove(&row);
        }

        let fields: Vec<Field> = handle
            .read(|project| {
                vec![
                    (
                        t("App name"),
                        t("The name people see under the icon."),
                        "Pack It Flat",
                        project.name.clone(),
                        (|project: &mut Project, value: String| project.name = value)
                            as fn(&mut Project, String),
                    ),
                    (
                        t("App ID"),
                        t("A name no other app can have: your website address backwards, \
                           then the app's name."),
                        "no.oyzmo.PackItFlat",
                        project.manifest.app_id.clone(),
                        |project, value| project.manifest.app_id = value.trim().to_string(),
                    ),
                    (
                        t("In one line"),
                        t("Shown under the app's name in app stores."),
                        "Strip metadata from files",
                        project.summary.clone(),
                        |project, value| project.summary = value,
                    ),
                    (
                        t("Developer"),
                        t("Your name, or the project's."),
                        "oyzmo",
                        project.developer.clone(),
                        |project, value| project.developer = value,
                    ),
                    (
                        t("Website"),
                        t("Where people can read more about it."),
                        "http://oyzmo.no",
                        project.homepage.clone(),
                        |project, value| project.homepage = value,
                    ),
                    (
                        t("Program to start"),
                        t("The program that runs when someone opens the app."),
                        "packitflat",
                        project.manifest.command.clone(),
                        |project, value| project.manifest.command = value.trim().to_string(),
                    ),
                ]
            });

        for (index, (title, subtitle, placeholder, value, apply)) in
            fields.into_iter().enumerate()
        {
            // The licence reads best right after the one-line summary, and it is
            // a picker rather than a box to type in: an SPDX identifier from
            // memory is exactly the thing beginners get subtly wrong, and the
            // metainfo an app store reads is where it ends up.
            if index == 3 {
                self.add_licence_row();
            }

            let handle = handle.clone();
            let (row, _) = forms::entry_row(&title, &subtitle, placeholder, &value, move |value| {
                handle.write(|project| apply(project, value));
            });
            imp.about_group.add(&row);
            imp.about_rows.borrow_mut().push(row);
        }
    }

    /// The licence, as a row that opens the same list the guided steps use. The
    /// label showing the choice is updated from the picker rather than from a
    /// redraw, because the manifest pane is built once and left alone.
    fn add_licence_row(&self) {
        let imp = self.imp();
        let handle = self.handle();

        let row = adw::ActionRow::builder()
            .title(t("Licence"))
            .subtitle(t("What other people are allowed to do with your code."))
            .subtitle_lines(0)
            .activatable(true)
            .build();

        let value = gtk::Label::builder()
            .label(handle.read(|project| forms::licence_summary(&project.license)))
            .valign(gtk::Align::Center)
            .build();
        value.add_css_class("dim-label");
        row.add_suffix(&value);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

        row.connect_activated(clone!(
            #[strong]
            handle,
            #[weak]
            value,
            move |row| {
                let current = handle.read(|project| project.license.clone());
                let handle = handle.clone();
                forms::choose_licence(row, &current, move |id| {
                    handle.write(|project| project.license = id.to_string());
                    value.set_label(&forms::licence_summary(id));
                });
            }
        ));

        imp.about_group.add(&row);
        imp.about_rows.borrow_mut().push(row);
    }

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

        if !current_runtime.is_empty()
            && runtimes::position_of(&choices, &current_runtime, &current_version).is_none()
        {
            choices.insert(
                0,
                runtimes::Choice {
                    sdk: runtimes::sdk_for(&current_runtime),
                    friendly: runtimes::friendly(&current_runtime, &current_version),
                    support: runtimes::support(&current_runtime, &current_version),
                    installed: false,
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

    fn load_build_pane(&self) {
        let imp = self.imp();
        let handle = self.handle();

        handle.silently(|| {
            let names: Vec<String> = forms::BUILD_SYSTEMS
                .iter()
                .map(|(_, label, _)| t(label))
                .collect();
            imp.buildsystem_combo
                .set_model(Some(&string_list(&names)));

            handle.read(|project| {
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
    }

    /// The permissions and appearance panes, built by `panes` — the same widgets
    /// the guided steps show, over the same project.
    fn load_panes(&self) {
        let imp = self.imp();
        let handle = self.handle();
        let rebuild = self.rebuild_panes();

        panes::install_permission_actions(self, &handle, rebuild.clone());
        panes::build_dependencies(&imp.dependencies_box, &handle, rebuild.clone());
        panes::build_permissions(&imp.permissions_box, &handle, rebuild.clone());
        panes::build_appearance(&imp.appearance_box, &handle, rebuild);
    }

    fn rebuild_panes(&self) -> Rc<dyn Fn()> {
        let editor = self.downgrade();
        Rc::new(move || {
            if let Some(editor) = editor.upgrade() {
                let handle = editor.handle();
                let imp = editor.imp();
                let again = editor.rebuild_panes();
                panes::build_dependencies(&imp.dependencies_box, &handle, again.clone());
                panes::build_permissions(&imp.permissions_box, &handle, again.clone());
                panes::build_appearance(&imp.appearance_box, &handle, again);
                editor.refresh();
            }
        })
    }

    fn load_yaml(&self) {
        let handle = self.handle();
        let text = handle.read(sync::text_for);
        handle.silently(|| self.imp().yaml_view.buffer().set_text(&text));
    }

    // -- reading back --------------------------------------------------------

    fn collect(&self) {
        let imp = self.imp();
        let handle = self.handle();
        if handle.is_muted() {
            return;
        }

        handle.write(|project| {
            if let Some(choice) = imp
                .runtime_choices
                .borrow()
                .get(imp.runtime_combo.selected() as usize)
            {
                project.manifest.runtime = choice.runtime.clone();
                project.manifest.runtime_version = choice.version.clone();
                project.manifest.sdk = choice.sdk.clone();
            }

            let name = project
                .manifest
                .main_module()
                .map(|module| module.name.clone())
                .unwrap_or_else(|| "app".to_string());
            let build = forms::build_system_at(imp.buildsystem_combo.selected());
            let commands = forms::lines_of(&*imp.commands_view);
            let options = forms::lines_of(&*imp.options_view);
            let env = forms::env_mapping(&forms::lines_of(&*imp.env_view));

            let module = project.manifest.ensure_main_module(&name);
            module.buildsystem = build;
            module.build_commands = commands;
            module.config_opts = options;
            forms::set_module_env(module, env);

            // As in the wizard: an icon already in place is this project's, and
            // a `simple` build installs exactly what its commands say.
            packitflat::icons::adopt_existing(project);
            packitflat::generate::sync_install_commands(project);
        });
    }

    /// What the user typed in the raw pane. On a parse failure the model is left
    /// alone and so is the text — only the banner changes.
    fn take_yaml(&self) {
        let imp = self.imp();
        let handle = self.handle();
        if handle.is_muted() {
            return;
        }

        let text = forms::text_of(&*imp.yaml_view);
        let outcome = {
            let project = handle.shared();
            let mut project = project.borrow_mut();
            sync::apply_text(&mut project, &text)
        };

        match outcome {
            sync::Outcome::Invalid(err) => {
                imp.error_position.set(sync::error_position(&err));
                imp.yaml_banner.set_title(&err.friendly());
                imp.yaml_banner
                    .set_button_label(sync::error_position(&err).map(|_| t("Go to it")).as_deref());
                imp.yaml_banner.set_revealed(true);
            }
            sync::Outcome::Unchanged => {
                imp.error_position.set(None);
                imp.yaml_banner.set_revealed(false);
            }
            sync::Outcome::Applied => {
                imp.error_position.set(None);
                imp.yaml_banner.set_revealed(false);
                // The forms have to follow the text, but the text must not be
                // reformatted underneath the cursor — so everything *except* the
                // YAML view is reloaded.
                self.reload_forms();
                self.refresh();
                self.save();
            }
        }
    }

    /// Put the caret where the parser gave up.
    fn jump_to_error(&self) {
        let imp = self.imp();
        let Some((line, column)) = imp.error_position.get() else {
            return;
        };
        let buffer = imp.yaml_view.buffer();
        // serde_yaml counts from 1; GtkTextBuffer from 0.
        let line = (line.saturating_sub(1)) as i32;
        let column = (column.saturating_sub(1)) as i32;
        if let Some(mut iter) = buffer.iter_at_line(line.min(buffer.line_count() - 1)) {
            let remaining = iter.chars_in_line();
            iter.set_line_offset(column.min(remaining.max(1) - 1).max(0));
            buffer.place_cursor(&iter);
            imp.yaml_view
                .scroll_to_iter(&mut iter, 0.1, false, 0.0, 0.5);
            imp.yaml_view.grab_focus();
        }
    }

    fn reload_forms(&self) {
        self.load_tree();
        self.load_manifest_pane();
        self.load_runtimes();
        self.load_extensions();
        self.load_sources();
        self.load_build_pane();
        self.load_panes();
    }

    // -- derived -------------------------------------------------------------

    fn refresh(&self) {
        let imp = self.imp();
        let handle = self.handle();
        let issues = handle.read(validate::project);

        // The SDK is chosen by the runtime, never separately; saying so is more
        // use than a second picker.
        handle.read(|project| {
            imp.sdk_row.set_subtitle(&if project.manifest.sdk.is_empty() {
                t("Chosen automatically from the runtime.")
            } else {
                format!(
                    "{} — {}",
                    project.manifest.sdk,
                    t("chosen automatically to match the runtime")
                )
            });
        });

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

        let errors = issues.errors();
        imp.write_button.set_sensitive(errors == 0);
        imp.write_button.set_tooltip_text(Some(&if errors == 0 {
            t("Write the manifest into the project folder")
        } else if errors == 1 {
            t("One thing still needs an answer — see Problems")
        } else {
            format!("{errors} things still need answers — see Problems")
        }));

        forms::clear(&imp.problems_list);
        for issue in &issues {
            let part = part_for(issue.field);
            let editor = self.downgrade();
            let go_there: Box<dyn Fn()> = Box::new(move || {
                if let Some(editor) = editor.upgrade() {
                    editor.show_part(&part);
                }
            });
            imp.problems_list
                .append(&forms::issue_row(issue, Some(go_there)));
        }
        imp.problems_group
            .set_title(&if issues.is_empty() {
                t("Nothing to fix")
            } else if errors == 0 {
                t("Suggestions")
            } else {
                t("Worth fixing")
            });
        if issues.is_empty() {
            let row = adw::ActionRow::builder()
                .title(t("Everything needed is filled in."))
                .subtitle(t("The manifest can be written whenever you like."))
                .subtitle_lines(0)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
            imp.problems_list.append(&row);
        }

        // The text pane follows the model, unless the text is mid-edit — see
        // packitflat::sync, where that rule lives and is tested.
        let current = forms::text_of(&*imp.yaml_view);
        if handle.read(|project| sync::should_replace(&current, project)) {
            self.load_yaml();
        }
    }

    /// Show the pane a field lives on. Used when something else in the app —
    /// the build page's checklist, say — wants to put the user in front of it.
    pub fn show_field(&self, field: Option<validate::Field>) {
        let part = field.map(part_for).unwrap_or(Part::Problems);
        self.show_part(&part);
    }

    fn show_part(&self, part: &Part) {
        let imp = self.imp();
        imp.panes.set_visible_child_name(part.pane());
        imp.detail_page.set_title(&part.title());
        imp.split.set_show_content(true);

        if let Part::OtherModule(name) = part {
            imp.other_module_page.set_title(name);
        }

        // Keep the sidebar's highlight in step when the jump came from a button.
        if let Some(index) = imp.parts.borrow().iter().position(|other| other == part) {
            if let Some(row) = imp.tree.row_at_index(index as i32) {
                if imp.tree.selected_row().as_ref() != Some(&row) {
                    imp.tree.select_row(Some(&row));
                }
            }
        }
    }

    /// Save at once, wherever waiting would be wrong: the page is going away.
    fn save(&self) {
        self.handle().save_now();
    }

    /// Drive the raw pane the way a person would and check what happens. The
    /// sync *policy* is unit-tested in `packitflat::sync`; this checks the wiring
    /// around it, which unit tests can't reach: that typing really does reach the
    /// model, and that a broken edit really does leave both the model and the
    /// text alone.
    #[cfg(debug_assertions)]
    pub fn dev_sync_check(&self) -> bool {
        let imp = self.imp();
        let handle = self.handle();
        let mut ok = true;

        let check = |what: &str, passed: bool| {
            eprintln!("sync-check: {} {what}", if passed { "ok  " } else { "FAIL" });
            passed
        };

        // A valid edit reaches the model, and the forms follow it.
        let edited = handle
            .read(sync::text_for)
            .replace("command: ", "command: typed-");
        imp.yaml_view.buffer().set_text(&edited);
        ok &= check(
            "typing valid YAML updates the project",
            handle.read(|project| project.manifest.command.starts_with("typed-")),
        );
        ok &= check("no error is reported", !imp.yaml_banner.is_revealed());

        // A broken edit changes nothing but the banner.
        let before = handle.read(|project| project.manifest.clone());
        let broken = "app-id: fine\n  bad: indent\n";
        imp.yaml_view.buffer().set_text(broken);
        ok &= check(
            "broken YAML leaves the project untouched",
            handle.read(|project| project.manifest == before),
        );
        ok &= check("the problem is reported", imp.yaml_banner.is_revealed());
        ok &= check(
            "the typed text is left alone",
            forms::text_of(&*imp.yaml_view).contains("bad: indent"),
        );

        // …including when something else asks for a redraw.
        self.refresh();
        ok &= check(
            "a redraw does not overwrite unparseable text",
            forms::text_of(&*imp.yaml_view).contains("bad: indent"),
        );

        // Putting it back leaves the project as it was before the broken edit.
        imp.yaml_view.buffer().set_text(&edited);
        ok &= check(
            "the model comes back when the text parses again",
            handle.read(|project| project.manifest == before),
        );

        eprintln!(
            "sync-check: {}",
            if ok { "all checks passed" } else { "FAILURES ABOVE" }
        );
        ok
    }

    /// Rebuild the forms the way an edit to the manifest text does.
    #[cfg(debug_assertions)]
    pub fn dev_reload_forms(&self) {
        self.reload_forms();
    }

    /// Every value the manifest pane is actually showing, for the harness.
    #[cfg(debug_assertions)]
    pub fn dev_manifest_fields(&self) -> Vec<String> {
        fn entries(widget: &gtk::Widget, out: &mut Vec<String>) {
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
                    out.push(entry.text().to_string());
                }
                entries(&widget, out);
                child = widget.next_sibling();
            }
        }
        let mut out = Vec::new();
        entries(self.imp().about_group.upcast_ref(), &mut out);
        out
    }

    /// Jump to a pane by the name the sidebar uses. Only the screenshot harness
    /// needs this; a person clicks the sidebar.
    #[cfg(debug_assertions)]
    pub fn dev_show_pane(&self, name: &str) {
        let part = self
            .imp()
            .parts
            .borrow()
            .iter()
            .find(|part| part.pane() == name)
            .cloned();
        if let Some(part) = part {
            self.show_part(&part);
        }
    }
}

fn part_for(field: validate::Field) -> Part {
    use validate::Field;
    match field {
        Field::Name
        | Field::AppId
        | Field::Summary
        | Field::Description
        | Field::License
        | Field::Homepage
        | Field::Developer
        | Field::Command
        | Field::Runtime => Part::Manifest,
        Field::Sources => Part::Sources,
        Field::BuildSystem => Part::Build,
        Field::Dependencies => Part::Dependencies,
        Field::Permissions => Part::Permissions,
        Field::Categories | Field::Icon | Field::Release => Part::Appearance,
    }
}

fn string_list(items: &[String]) -> gtk::StringList {
    let list = gtk::StringList::new(&[]);
    for item in items {
        list.append(item);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_problem_leads_somewhere_in_the_sidebar() {
        for field in [
            validate::Field::Name,
            validate::Field::AppId,
            validate::Field::Summary,
            validate::Field::Description,
            validate::Field::License,
            validate::Field::Homepage,
            validate::Field::Developer,
            validate::Field::Runtime,
            validate::Field::Sources,
            validate::Field::BuildSystem,
            validate::Field::Command,
            validate::Field::Permissions,
            validate::Field::Categories,
            validate::Field::Icon,
            validate::Field::Release,
        ] {
            let part = part_for(field);
            assert!(
                !part.pane().is_empty() && !part.title().is_empty(),
                "{field:?} has nowhere to go"
            );
        }
    }
}
