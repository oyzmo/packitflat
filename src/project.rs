//! The project: a source folder plus the manifest being written for it, and
//! the on-disk store that lets a half-finished setup survive a crash.
//!
//! Wizard mode, editor mode and the raw YAML view are all views over this one
//! value. Nothing may keep its own copy of the manifest — switching views must
//! never lose data, and that is only guaranteed if there is nothing to sync.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::detect::{self, Detection, ProjectKind};
use crate::manifest::{Import, Manifest, ModuleEntry, SourceEntry};

/// A picture for the app's store page. Stores fetch these over the web, so it is
/// an address rather than a file on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Screenshot {
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Project {
    /// What the user calls the app. Not necessarily the module or binary name.
    pub name: String,
    /// The one-line description shown under the name in app stores. These four
    /// fields are not manifest keys — they belong to the AppStream metainfo the
    /// generator writes later — but they are asked for on the first step, so
    /// they live with the rest of the project rather than in a second model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// An SPDX identifier, e.g. "GPL-3.0-or-later".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub homepage: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub developer: String,

    /// Freedesktop category names, for the menu entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// The picture to use, as the user chose it. Copied into the project's own
    /// icons folder when the files are generated, never referenced from here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_source: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshots: Vec<Screenshot>,
    /// Answers to the content-rating questions, as (question id, answer index).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_rating: Vec<(String, usize)>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release_version: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release_date: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub release_notes: String,
    /// The folder being packaged, if the project came from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_dir: Option<PathBuf>,
    /// Where an imported manifest was read from, so "save" can offer it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<PathBuf>,
    /// What the folder scan concluded, remembered so the summary can repeat it
    /// without rescanning a folder that may have gone away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected: Option<String>,
    pub manifest: Manifest,
}

impl Project {
    /// A project started by pointing at a folder. The manifest gets working
    /// defaults and one module named after the folder; the app ID is left for
    /// the user, because guessing a domain they don't own is worse than asking.
    pub fn from_folder(dir: &Path) -> (Self, Detection) {
        let dir = detect::canonical(dir);
        let detection = detect::detect(&dir);
        let name = detection.suggested_name.clone();
        // The program the manifest starts: what the project calls it, and only
        // the folder's name as a last resort.
        let command = detection.binary.clone().unwrap_or_else(|| name.clone());

        let mut manifest = Manifest::starter("", &command);
        manifest.sdk_extensions = detection.kind.sdk_extensions();
        // The compiler the detection just asked for has to be findable, or the
        // build downloads everything and then fails on "command not found".
        crate::runtimes::sync_build_paths(&mut manifest);

        let mut module = crate::manifest::Module::new(&name);
        if detection.kind != ProjectKind::Unknown && detection.kind.buildsystem() != "simple" {
            module.buildsystem = Some(detection.kind.buildsystem().to_string().into());
        }
        module.build_commands = detection.build_commands.clone();
        module
            .sources
            .push(SourceEntry::Source(crate::manifest::Source::dir(".")));
        manifest.modules.push(ModuleEntry::Module(module));

        let project = Project {
            name: title_case(&name),
            source_dir: Some(dir),
            imported_from: None,
            detected: Some(detection.kind.title().to_string()),
            manifest,
            ..Project::blank()
        };
        (project, detection)
    }

    /// A project built from someone else's manifest.
    pub fn from_import(import: &Import, path: Option<&Path>) -> Self {
        let manifest = import.manifest.clone();
        let name = manifest
            .app_id
            .rsplit('.')
            .next()
            .filter(|s| !s.is_empty())
            .map(spaced_case)
            .unwrap_or_else(|| "Imported project".to_string());

        Project {
            name,
            source_dir: path.and_then(|p| p.parent()).map(Path::to_path_buf),
            imported_from: path.map(Path::to_path_buf),
            detected: None,
            manifest,
            ..Project::blank()
        }
    }

    /// Everything empty. Only used as the tail of a struct update, so adding a
    /// field to `Project` can't silently skip one of the constructors.
    fn blank() -> Self {
        Project {
            name: String::new(),
            summary: String::new(),
            description: String::new(),
            license: String::new(),
            homepage: String::new(),
            developer: String::new(),
            categories: Vec::new(),
            keywords: Vec::new(),
            icon_source: None,
            screenshots: Vec::new(),
            content_rating: Vec::new(),
            release_version: String::new(),
            release_date: String::new(),
            release_notes: String::new(),
            source_dir: None,
            imported_from: None,
            detected: None,
            manifest: Manifest::default(),
        }
    }

    /// File name the project is stored under. Derived, never user-entered, so
    /// it can't collide with a path the user typed.
    pub fn slug(&self) -> String {
        // Named after the folder, because that is what a project *is*. Naming it
        // after the app ID meant that filling the ID in later saved a second
        // copy under a new name and left the old one on the recent list —
        // switching between the two modes could leave a trail of them.
        let base = self
            .source_dir
            .as_ref()
            .or(self.imported_from.as_ref())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| {
                if self.manifest.app_id.is_empty() {
                    self.name.clone()
                } else {
                    self.manifest.app_id.clone()
                }
            });

        let slug: String = base
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '-' })
            .collect();
        let slug = slug.trim_matches('-').to_lowercase();
        if slug.is_empty() {
            "untitled".to_string()
        } else {
            slug
        }
    }

    /// The one-line description under the project's name in a list.
    pub fn subtitle(&self) -> String {
        match (&self.source_dir, &self.imported_from) {
            (_, Some(path)) => path.display().to_string(),
            (Some(dir), _) => dir.display().to_string(),
            _ => String::new(),
        }
    }
}

/// `~/.local/share/packitflat/projects`, or the Flatpak equivalent — inside the
/// sandbox XDG_DATA_HOME already points at the app's own data directory, so the
/// same code is right in both places.
pub fn projects_dir() -> PathBuf {
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".local/share")
        });
    data_home.join("packitflat").join("projects")
}

/// Autosave. Writes through a temp file and a rename, so an interrupted save
/// can't leave a half-written project behind — the thing this exists to prevent.
pub fn save_in(dir: &Path, project: &Project) -> Result<PathBuf> {
    fs::create_dir_all(dir)
        .with_context(|| format!("could not create {}", dir.display()))?;

    let target = dir.join(format!("{}.yml", project.slug()));
    let temp = dir.join(format!(".{}.tmp", project.slug()));
    let text = serde_yaml_ng::to_string(project).context("could not write the project as YAML")?;

    fs::write(&temp, text).with_context(|| format!("could not write {}", temp.display()))?;
    fs::rename(&temp, &target)
        .with_context(|| format!("could not save to {}", target.display()))?;
    Ok(target)
}

pub fn save(project: &Project) -> Result<PathBuf> {
    save_in(&projects_dir(), project)
}

/// Forget a project: delete the app's own saved copy, and nothing else. The
/// user's folder, their code and any manifest already written are untouched —
/// this only removes the entry from the recent list.
pub fn forget(path: &Path) -> Result<()> {
    // Only ever this app's own files, whatever it is handed.
    // `is_none_or` would read better, but it needs a newer Rust than this crate
    // claims to support.
    if path.extension().map(|extension| extension != "yml") != Some(false)
        || path.parent() != Some(projects_dir().as_path())
    {
        anyhow::bail!(
            "{} isn't one of this app's saved projects",
            path.display()
        );
    }

    fs::remove_file(path).with_context(|| format!("could not remove {}", path.display()))
}

pub fn load(path: &Path) -> Result<Project> {
    let text =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    serde_yaml_ng::from_str(&text)
        .with_context(|| format!("{} is not a saved project", path.display()))
}

#[derive(Debug, Clone)]
pub struct Recent {
    pub name: String,
    pub subtitle: String,
    pub path: PathBuf,
    pub modified: SystemTime,
}

/// Recently worked-on projects, newest first. A file that no longer parses is
/// skipped rather than reported: the welcome page is not the place to explain a
/// corrupt autosave, and the user can still start a new project.
pub fn recents_in(dir: &Path) -> Vec<Recent> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Recent> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "yml"))
        .filter_map(|e| {
            let path = e.path();
            let project = load(&path).ok()?;
            let modified = e.metadata().ok()?.modified().ok()?;
            Some(Recent {
                name: project.name.clone(),
                subtitle: project.subtitle(),
                path,
                modified,
            })
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.modified));

    // One entry per folder, newest kept. Saved copies from older versions of
    // this app were named after the app ID, so the same project can be on disk
    // twice; the list shouldn't show it twice.
    let mut seen: Vec<String> = Vec::new();
    out.retain(|recent| {
        let key = recent.subtitle.clone();
        if key.is_empty() {
            return true;
        }
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });
    out
}

pub fn recents() -> Vec<Recent> {
    recents_in(&projects_dir())
}

fn title_case(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "PackItFlat" -> "Pack It Flat". Used on the last segment of an app ID, which
/// is camel case by convention.
fn spaced_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    #[test]
    fn folder_project_is_ready_to_build_on() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("my-thing");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("Cargo.toml"), "").unwrap();

        let (project, detection) = Project::from_folder(&src);
        assert_eq!(detection.kind, ProjectKind::Rust);
        assert_eq!(project.name, "My Thing");
        let module = project.manifest.main_module().unwrap();
        assert_eq!(module.name, "my-thing");
        assert_eq!(
            module.sources[0].as_source().unwrap().path.as_deref(),
            Some(".")
        );
        assert!(project
            .manifest
            .sdk_extensions
            .iter()
            .any(|e| e.contains("rust-stable")));
        // The app ID is deliberately not guessed.
        assert!(project.manifest.app_id.is_empty());
    }

    #[test]
    fn imported_project_takes_its_name_from_the_app_id() {
        let import = manifest::parse_str("app-id: no.oyzmo.PackItFlat\n").unwrap();
        let project = Project::from_import(&import, Some(Path::new("/tmp/x/thing.yml")));
        assert_eq!(project.name, "Pack It Flat");
        assert_eq!(project.imported_from.unwrap(), Path::new("/tmp/x/thing.yml"));
    }

    #[test]
    fn save_and_reload_is_lossless() {
        let dir = tempfile::tempdir().unwrap();
        let import = manifest::parse_str(
            "app-id: no.oyzmo.X\nruntime-version: '50'\ncleanup: [/include]\n",
        )
        .unwrap();
        let project = Project::from_import(&import, None);

        let path = save_in(dir.path(), &project).unwrap();
        assert_eq!(path.file_name().unwrap(), "no.oyzmo.x.yml");
        assert_eq!(load(&path).unwrap(), project);
    }

    /// The bug this prevents: saving a project, filling in the app ID, saving
    /// again, and finding two of it on the welcome page.
    #[test]
    fn filling_in_the_app_id_later_does_not_leave_a_second_copy() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("my-app");
        fs::create_dir(&folder).unwrap();

        let (mut project, _) = Project::from_folder(&folder);
        let before = save_in(dir.path(), &project).unwrap();

        project.manifest.app_id = "no.oyzmo.MyApp".into();
        project.name = "Something Else".into();
        let after = save_in(dir.path(), &project).unwrap();

        assert_eq!(before, after, "the same project keeps the same file");
        assert_eq!(recents_in(dir.path()).len(), 1);
    }

    #[test]
    fn two_saved_copies_of_one_folder_show_as_one_entry() {
        let dir = tempfile::tempdir().unwrap();
        let import = manifest::parse_str("app-id: no.oyzmo.A\n").unwrap();
        let mut project = Project::from_import(&import, None);
        project.source_dir = Some(std::path::PathBuf::from("/home/me/code/thing"));

        // As an older version would have written it, beside the new name.
        fs::write(
            dir.path().join("no.oyzmo.a.yml"),
            serde_yaml_ng::to_string(&project).unwrap(),
        )
        .unwrap();
        save_in(dir.path(), &project).unwrap();

        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        assert_eq!(recents_in(dir.path()).len(), 1, "but only one is offered");
    }

    #[test]
    fn forgetting_a_project_removes_only_this_apps_own_copy() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("someones-real-manifest.yml");
        fs::write(&elsewhere, "app-id: no.oyzmo.Precious\n").unwrap();

        // Anything that isn't one of this app's saved projects is refused,
        // whatever it is handed: the recent list must never be able to delete
        // somebody's work.
        assert!(forget(&elsewhere).is_err());
        assert!(elsewhere.is_file());
        assert!(forget(&projects_dir().join("no.oyzmo.X.txt")).is_err());
    }

    #[test]
    fn recents_are_newest_first_and_skip_junk() {
        let dir = tempfile::tempdir().unwrap();
        let import = manifest::parse_str("app-id: no.oyzmo.A\n").unwrap();
        save_in(dir.path(), &Project::from_import(&import, None)).unwrap();
        fs::write(dir.path().join("broken.yml"), "not: [a project").unwrap();

        let recents = recents_in(dir.path());
        assert_eq!(recents.len(), 1);
        assert_eq!(recents[0].name, "A");
    }

    #[test]
    fn projects_dir_follows_xdg() {
        let dir = projects_dir();
        assert!(dir.ends_with("packitflat/projects"));
        assert!(dir.is_absolute());
    }
}
