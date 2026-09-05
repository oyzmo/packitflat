//! Starting points, for when there is no project to point at yet.
//!
//! Each one is a project that would build if you gave it code: runtime, build
//! system, commands, permissions and the tools that kind of app needs. They
//! exist so the first screen has an answer for "I haven't got anything yet",
//! and so nobody has to discover on their own that a Rust app needs the
//! rust-stable extension and `CARGO_HOME` pointed somewhere sensible.

use std::path::{Path, PathBuf};

use crate::detect::ProjectKind;
use crate::manifest::{BuildSystem, Manifest, Module, ModuleEntry, Source, SourceEntry};
use crate::project::Project;
use crate::vendor;

pub struct Template {
    pub id: &'static str,
    pub name: &'static str,
    /// One line: who this is for.
    pub description: &'static str,
    /// What choosing it fills in.
    pub detail: &'static str,
    pub kind: ProjectKind,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "gtk-rust",
        name: "A GNOME app written in Rust",
        description: "GTK4 and libadwaita, built with cargo.",
        detail: "Sets the GNOME runtime, adds the Rust compiler to the build, writes the \
                 cargo build and install commands, and asks for the permissions a windowed \
                 app needs.",
        kind: ProjectKind::Rust,
    },
    Template {
        id: "gtk-python",
        name: "A GNOME app written in Python",
        description: "GTK4 and libadwaita, installed with Meson.",
        detail: "Sets the GNOME runtime and a Meson build. Python itself is already in \
                 the runtime, so there is nothing to add for it.",
        kind: ProjectKind::Meson,
    },
    Template {
        id: "gtk-c",
        name: "A GNOME app written in C",
        description: "GTK4 and libadwaita, built with Meson.",
        detail: "Sets the GNOME runtime and a Meson build — the usual arrangement for a \
                 GTK app in C or Vala.",
        kind: ProjectKind::Meson,
    },
    Template {
        id: "electron",
        name: "An Electron or Node app",
        description: "A JavaScript app that ships its own browser.",
        detail: "Sets the Freedesktop runtime, adds Node to the build, and asks for the \
                 extra permissions Electron needs to draw and to reach the network.",
        kind: ProjectKind::Node,
    },
    Template {
        id: "autotools",
        name: "An older program with configure and make",
        description: "Anything that builds the classic way.",
        detail: "Sets the Freedesktop runtime and lets Flatpak drive configure and make \
                 for you.",
        kind: ProjectKind::Autotools,
    },
    Template {
        id: "wrap-binary",
        name: "Wrap a program I already have",
        description: "Something already compiled, put into a Flatpak as it is.",
        detail: "Copies the program in and installs it, with no compiler involved. Good \
                 for a script, or for a binary somebody else built.",
        kind: ProjectKind::Unknown,
    },
];

pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

/// The runtime a template starts on. GNOME for anything with a window that uses
/// GTK; Freedesktop for everything else, because it is the smallest thing that
/// still has a C library and a shell.
fn runtime_for(template: &Template) -> (&'static str, &'static str, &'static str) {
    match template.id {
        "electron" | "autotools" | "wrap-binary" => {
            ("org.freedesktop.Platform", "org.freedesktop.Sdk", "25.08")
        }
        _ => ("org.gnome.Platform", "org.gnome.Sdk", "50"),
    }
}

/// Build a project from a template. `folder` is where the code will live; when
/// there isn't one yet, the project is still complete apart from that.
pub fn apply(template: &Template, name: &str, folder: Option<&Path>) -> Project {
    let module_name = slug(name);
    let command = module_name.clone();
    let (runtime, sdk, version) = runtime_for(template);

    let mut manifest = Manifest::starter("", &command);
    manifest.runtime = runtime.to_string();
    manifest.sdk = sdk.to_string();
    manifest.runtime_version = version.to_string();
    manifest.sdk_extensions = template.kind.sdk_extensions();
    crate::runtimes::sync_build_paths(&mut manifest);

    if template.id == "electron" {
        // Electron draws its own everything and expects to reach the network.
        manifest.finish_args.push("--share=network".into());
        manifest.finish_args.push("--socket=pulseaudio".into());
    }

    let mut module = Module::new(&module_name);
    module.buildsystem = build_system_for(template);
    module.build_commands = build_commands_for(template, &command);
    module
        .sources
        .push(SourceEntry::Source(Source::dir(".")));
    manifest.modules.push(ModuleEntry::Module(module));

    if template.kind == ProjectKind::Rust {
        // Rust cannot build offline without the crate list, and this is the one
        // place the app knows for certain that it will be needed.
        vendor::wire_in(&mut manifest, vendor::Ecosystem::Cargo);
    }

    Project {
        name: name.to_string(),
        summary: String::new(),
        description: String::new(),
        license: "GPL-3.0-or-later".to_string(),
        homepage: String::new(),
        developer: String::new(),
        categories: categories_for(template),
        keywords: Vec::new(),
        icon_source: None,
        screenshots: Vec::new(),
        content_rating: Vec::new(),
        release_version: "0.1.0".to_string(),
        release_date: crate::appdata::today(),
        release_notes: "First release.".to_string(),
        source_dir: folder.map(Path::to_path_buf),
        imported_from: None,
        detected: Some(format!("Started from “{}”", template.name)),
        manifest,
    }
}

fn build_system_for(template: &Template) -> Option<BuildSystem> {
    match template.id {
        "gtk-python" | "gtk-c" => Some(BuildSystem::Meson),
        "autotools" => Some(BuildSystem::Autotools),
        _ => Some(BuildSystem::Simple),
    }
}

fn build_commands_for(template: &Template, command: &str) -> Vec<String> {
    match template.id {
        "gtk-rust" => vec![
            "cargo build --release".to_string(),
            format!("install -Dm755 target/release/{command} /app/bin/{command}"),
        ],
        "electron" => vec![
            "npm install --offline".to_string(),
            "npm run build".to_string(),
            format!(
                "install -Dm755 run.sh /app/bin/{command}"
            ),
        ],
        "wrap-binary" => vec![format!("install -Dm755 {command} /app/bin/{command}")],
        // Meson and autotools drive themselves.
        _ => Vec::new(),
    }
}

fn categories_for(template: &Template) -> Vec<String> {
    match template.id {
        "gtk-rust" | "gtk-python" | "gtk-c" => vec!["Utility".to_string()],
        "electron" => vec!["Network".to_string()],
        _ => Vec::new(),
    }
}

/// A name usable as a module name and a program name: lower case, no spaces.
fn slug(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() {
        "app".to_string()
    } else {
        cleaned
    }
}

/// Where a template's files would go, for the "and put it here" part of the
/// welcome page.
pub fn suggested_folder(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join("Projects").join(slug(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    #[test]
    fn every_template_explains_itself() {
        assert_eq!(TEMPLATES.len(), 6);
        for template in TEMPLATES {
            assert!(!template.name.is_empty());
            assert!(template.description.ends_with('.'), "{}", template.id);
            assert!(template.detail.ends_with('.'), "{}", template.id);
            assert!(find(template.id).is_some());
        }
        assert!(find("nonsense").is_none());
    }

    #[test]
    fn a_template_leaves_only_the_app_id_to_answer() {
        for template in TEMPLATES {
            let mut project = apply(template, "My App", Some(Path::new("/tmp/my-app")));
            project.manifest.app_id = "no.oyzmo.MyApp".into();

            let errors: Vec<_> = validate::project(&project)
                .into_iter()
                .filter(|issue| issue.severity == validate::Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "{} still has errors: {errors:#?}",
                template.id
            );
        }
    }

    #[test]
    fn without_an_app_id_that_is_the_only_thing_blocking() {
        for template in TEMPLATES {
            let project = apply(template, "My App", None);
            let blocking: Vec<_> = validate::project(&project)
                .into_iter()
                .filter(|issue| issue.severity == validate::Severity::Error)
                .collect();
            assert_eq!(blocking.len(), 1, "{}: {blocking:#?}", template.id);
            assert_eq!(blocking[0].field, validate::Field::AppId);
        }
    }

    #[test]
    fn the_rust_template_arrives_ready_to_build_offline() {
        let project = apply(find("gtk-rust").unwrap(), "My App", None);
        let manifest = &project.manifest;

        assert!(manifest
            .sdk_extensions
            .iter()
            .any(|extension| extension.contains("rust-stable")));
        assert!(vendor::is_wired(manifest, "cargo-sources.json"));

        let module = manifest.main_module().unwrap();
        assert!(module.build_commands[0].contains("cargo --offline fetch"));
        assert!(module
            .build_options
            .as_ref()
            .unwrap()
            .env
            .contains_key("CARGO_HOME"));
        assert_eq!(manifest.command, "my-app");
    }

    #[test]
    fn build_systems_that_drive_themselves_get_no_commands() {
        for id in ["gtk-python", "gtk-c", "autotools"] {
            let project = apply(find(id).unwrap(), "Thing", None);
            let module = project.manifest.main_module().unwrap();
            assert!(module.build_commands.is_empty(), "{id}");
            assert!(module.buildsystem.is_some(), "{id}");
        }
    }

    #[test]
    fn electron_asks_for_what_electron_needs_and_nothing_more() {
        let project = apply(find("electron").unwrap(), "Chatty", None);
        let args = &project.manifest.finish_args;
        assert!(args.iter().any(|arg| arg == "--share=network"));
        assert!(args.iter().any(|arg| arg == "--socket=pulseaudio"));
        assert!(!args.iter().any(|arg| arg.contains("filesystem=home")));
        assert_eq!(project.manifest.runtime, "org.freedesktop.Platform");
    }

    #[test]
    fn wrapping_a_binary_involves_no_compiler_at_all() {
        let project = apply(find("wrap-binary").unwrap(), "Old Tool", None);
        let module = project.manifest.main_module().unwrap();
        assert!(project.manifest.sdk_extensions.is_empty());
        assert_eq!(module.build_commands.len(), 1);
        assert!(module.build_commands[0].contains("/app/bin/old-tool"));
    }

    #[test]
    fn what_a_template_produces_is_a_manifest_this_app_can_read_back() {
        for template in TEMPLATES {
            let mut project = apply(template, "Round Trip", None);
            project.manifest.app_id = "no.oyzmo.RoundTrip".into();

            let text = project.manifest.to_yaml().unwrap();
            let reimported = crate::manifest::parse_str(&text).unwrap();
            assert_eq!(reimported.manifest, project.manifest, "{}", template.id);
            // Nothing it writes is beyond what it can edit. The Rust template
            // does produce one note — the crate list is an include, and the
            // import says so — but that is information, not a warning.
            assert_eq!(
                reimported.report.warnings(),
                0,
                "{}: {:#?}",
                template.id,
                reimported.report
            );
        }
    }

    #[test]
    fn names_become_something_usable_as_a_program_name() {
        assert_eq!(slug("My Cool App!"), "my-cool-app");
        assert_eq!(slug("  "), "app");
        assert!(suggested_folder("My App")
            .unwrap()
            .ends_with("Projects/my-app"));
    }
}
