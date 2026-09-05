//! Writing the project's files into the user's folder.
//!
//! Three rules: work out everything that would happen *before* anything happens,
//! never overwrite without saying so first, and never leave a half-written file
//! behind. The plan is what the review step shows — a list of files with a line
//! each saying what the file is for — so nothing appears on disk that the user
//! hasn't already seen described.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::project::Project;
use crate::{appdata, icons, VERSION};

#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("This project has no app ID yet, and every file is named after it.")]
    NoAppId,
    #[error("This project isn't linked to a folder, so there's nowhere to write anything.")]
    NoFolder,
    #[error("{0}")]
    Failed(String),
}

impl GenerateError {
    /// One line, safe to show as-is.
    pub fn friendly(&self) -> String {
        self.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Manifest,
    Desktop,
    Metainfo,
    Icon,
}

impl FileKind {
    /// The "what this file does" line the review step shows beside each one.
    pub fn purpose(&self) -> &'static str {
        match self {
            FileKind::Manifest => {
                "The recipe: what your app is built from, and what it may do once installed."
            }
            FileKind::Desktop => {
                "Puts the app in the menu, with its name and icon, and starts it when clicked."
            }
            FileKind::Metainfo => {
                "What app stores show: the description, the licence, the screenshots and the \
                 age rating."
            }
            FileKind::Icon => "The app's picture, copied to where Flatpak looks for it.",
        }
    }
}

/// One file the run would produce.
#[derive(Debug, Clone)]
pub struct PlannedFile {
    pub kind: FileKind,
    /// Where it goes.
    pub path: PathBuf,
    /// The same thing said shortly, relative to the project folder.
    pub relative: PathBuf,
    pub replaces_existing: bool,
    /// Unticking a file leaves it alone entirely.
    pub include: bool,
    /// Anything worth knowing about this particular file.
    pub note: Option<String>,
}

impl PlannedFile {
    pub fn file_name(&self) -> String {
        self.relative
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub folder: PathBuf,
    pub files: Vec<PlannedFile>,
}

impl Plan {
    pub fn manifest(&self) -> &PlannedFile {
        self.files
            .iter()
            .find(|file| file.kind == FileKind::Manifest)
            .expect("a plan always contains the manifest")
    }

    /// Whether anything in the plan would land on top of an existing file — what
    /// the "keep a copy" switch is about.
    pub fn replaces_anything(&self) -> bool {
        self.files
            .iter()
            .any(|file| file.include && file.replaces_existing)
    }

    pub fn included(&self) -> impl Iterator<Item = &PlannedFile> {
        self.files.iter().filter(|file| file.include)
    }

    pub fn set_included(&mut self, kind: FileKind, include: bool) {
        for file in self.files.iter_mut().filter(|file| file.kind == kind) {
            file.include = include;
        }
    }
}

pub fn plan(project: &Project) -> Result<Plan, GenerateError> {
    let app_id = project.manifest.app_id.trim();
    if app_id.is_empty() {
        return Err(GenerateError::NoAppId);
    }
    let folder = project.source_dir.clone().ok_or(GenerateError::NoFolder)?;

    let mut files = Vec::new();
    let mut add = |kind: FileKind, relative: PathBuf, note: Option<String>| {
        let path = folder.join(&relative);
        files.push(PlannedFile {
            kind,
            replaces_existing: path.exists(),
            path,
            relative,
            include: true,
            note,
        });
    };

    add(FileKind::Manifest, PathBuf::from(format!("{app_id}.yml")), None);
    add(
        FileKind::Desktop,
        PathBuf::from(appdata::desktop_file_name(app_id)),
        None,
    );
    add(
        FileKind::Metainfo,
        PathBuf::from(appdata::metainfo_file_name(app_id)),
        // Written either way, and said out loud when it won't be part of the
        // app: silently leaving it out would be the same kind of surprise as
        // silently failing the build with it in.
        (!appdata::listing_is_complete(project)).then(|| {
            "Written, but left out of the build until the app has a short \
             description, a longer description and a category. App stores refuse a \
             listing without all three, and the build fails with it."
                .to_string()
        }),
    );

    if let Some(source) = &project.icon_source {
        match icons::inspect(source) {
            Ok(kind) => {
                let note = icons::advice(&kind).first().cloned();
                add(FileKind::Icon, icons::target_path(app_id, &kind), note);
            }
            Err(err) => {
                // The picture is unusable, but that is not a reason to refuse to
                // write everything else — say so on the row and carry on.
                files.push(PlannedFile {
                    kind: FileKind::Icon,
                    path: source.clone(),
                    relative: source.clone(),
                    replaces_existing: false,
                    include: false,
                    note: Some(err.friendly()),
                });
            }
        }
    }

    Ok(Plan { folder, files })
}

/// Where each generated file has to end up inside the finished app.
///
/// `/app` is the prefix a Flatpak build installs into, and these are the paths
/// the desktop, the icon theme and AppStream look in. Nothing finds a file that
/// is merely *in* the app.
fn install_destination(kind: FileKind, app_id: &str, relative: &Path) -> Option<String> {
    match kind {
        FileKind::Manifest => None,
        FileKind::Desktop => Some(format!("/app/share/applications/{app_id}.desktop")),
        FileKind::Metainfo => Some(format!("/app/share/metainfo/{app_id}.metainfo.xml")),
        // The icon keeps the icon-theme layout it was written in, because that
        // is what makes it resolve by name.
        FileKind::Icon => Some(format!("/app/share/{}", relative.display())),
    }
}

/// The `install` lines a hand-written build needs so the app is more than a
/// binary.
///
/// **flatpak-builder installs nothing by itself for a `simple` module**: the
/// build-commands are the entire build. So a Rust or Node project takes the
/// commands `detect` suggested, installs its program, and produces a Flatpak
/// with no menu entry, no icon and no store listing — `share/` holds only the
/// licence. Nothing complains: the build succeeds, the app installs, and the
/// only symptom is a generic icon in a Flatpak manager, or an app that never
/// appears in the menu at all. (A real build did exactly this.)
///
/// The other build systems are left alone — there the project's own
/// `install()` rules do this, and `validate` says so when they are missing.
pub fn install_commands(project: &Project) -> Vec<String> {
    let app_id = project.manifest.app_id.trim();
    if app_id.is_empty() {
        return Vec::new();
    }
    if !matches!(
        project.manifest.main_module().and_then(|m| m.buildsystem.clone()),
        None | Some(crate::manifest::BuildSystem::Simple)
    ) {
        return Vec::new();
    }
    let Ok(plan) = plan(project) else {
        return Vec::new();
    };

    plan.files
        .iter()
        .filter(|file| file.include)
        .filter_map(|file| {
            let destination = install_destination(file.kind, app_id, &file.relative)?;
            Some(format!(
                "install -Dm644 {} {destination}",
                file.relative.display()
            ))
        })
        .collect()
}

/// Keep those lines in the manifest, without disturbing anything the user wrote.
///
/// A line is judged by where it installs *to*, so editing the source path on the
/// left keeps working and nothing is ever added twice.
pub fn sync_install_commands(project: &mut Project) {
    let Ok(plan) = plan(project) else {
        return;
    };
    sync_install_commands_with(project, &plan);
}

/// Whether the build is handed the folder these files are written into.
///
/// A `dir` source is: the build copies that folder, so anything written beside
/// the manifest is in the build. Git and archive sources are not.
fn module_takes_a_local_folder(project: &Project) -> bool {
    project.manifest.main_module().is_some_and(|module| {
        module.sources.iter().any(|entry| {
            entry
                .as_source()
                .is_some_and(|source| source.kind == crate::manifest::SourceKind::Dir)
        })
    })
}

/// The same against a plan somebody has since changed — the review step's
/// switches, in practice.
///
/// It works both ways round, because a line is a promise that the file is
/// there: a file that will be written gets its line, and a file that has been
/// switched off has its line taken away again. Leaving the line behind would
/// fail the build on a missing file, which is a baffling way to be told that
/// you unticked something.
pub fn sync_install_commands_with(project: &mut Project, plan: &Plan) {
    let app_id = project.manifest.app_id.trim().to_string();
    if app_id.is_empty() {
        return;
    }
    if !matches!(
        project.manifest.main_module().and_then(|m| m.buildsystem.clone()),
        None | Some(crate::manifest::BuildSystem::Simple)
    ) {
        return;
    }

    // An unfinished listing is written, but kept out of the build: installed, it
    // would be handed to `appstreamcli compose`, refused, and the whole build
    // would fail on it. See `appdata::listing_is_complete`.
    let listing = appdata::listing_is_complete(project);

    let mut wanted = Vec::new();
    let mut unwanted = Vec::new();
    for file in &plan.files {
        let Some(destination) = install_destination(file.kind, &app_id, &file.relative) else {
            continue;
        };
        if file.include && (file.kind != FileKind::Metainfo || listing) {
            wanted.push(format!(
                "install -Dm644 {} {destination}",
                file.relative.display()
            ));
        } else {
            unwanted.push(destination);
        }
    }

    // Which of those files the build will actually be able to see. A `dir`
    // source hands the build the folder these files are written into, so they
    // are simply there. A git or archive source does not: the build fetches the
    // code from somewhere else, and a file this app wrote a moment ago exists
    // only on this computer. The build then compiles everything and stops on
    // `install: cannot stat 'app.desktop'` — measured, against this app's own
    // repository. Each one becomes a `file` source so it travels with the
    // manifest.
    let carried: Vec<(PathBuf, Option<String>)> = if module_takes_a_local_folder(project) {
        Vec::new()
    } else {
        plan.files
            .iter()
            .filter(|file| file.include && file.kind != FileKind::Manifest)
            .filter(|file| install_destination(file.kind, &app_id, &file.relative).is_some())
            .map(|file| {
                let dest = file
                    .relative
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| parent.display().to_string());
                (file.relative.clone(), dest)
            })
            .collect()
    };

    let Some(module) = project.manifest.main_module_mut() else {
        return;
    };
    // Nothing to add to a build that has no commands of its own yet: the program
    // itself isn't installed either, and inventing half a build would be worse.
    if module.build_commands.is_empty() {
        return;
    }

    for (relative, dest) in carried {
        let path = relative.display().to_string();
        let already = module.sources.iter().any(|entry| {
            entry
                .as_source()
                .and_then(|source| source.path.as_deref())
                .is_some_and(|existing| existing == path)
        });
        if !already {
            module
                .sources
                .push(crate::manifest::SourceEntry::Source(
                    crate::manifest::Source::file(path, dest),
                ));
        }
    }

    // A line is judged by where it installs *to*, so editing the source path on
    // the left keeps working and nothing is ever added twice.
    module
        .build_commands
        .retain(|existing| !unwanted.iter().any(|gone| existing.contains(gone)));

    for command in wanted {
        let Some(destination) = command.split_whitespace().next_back() else {
            continue;
        };
        if module
            .build_commands
            .iter()
            .any(|existing| existing.contains(destination))
        {
            continue;
        }
        module.build_commands.push(command);
    }
}

#[derive(Debug, Clone)]
pub struct Written {
    pub kind: FileKind,
    pub path: PathBuf,
    pub backup_path: Option<PathBuf>,
}

/// Write everything the plan includes. With `backup`, an existing file is copied
/// aside first — copied rather than moved, so a failure halfway through leaves
/// the original where it was.
pub fn write(project: &Project, plan: &Plan, backup: bool) -> Result<Vec<Written>, GenerateError> {
    let app_id = project.manifest.app_id.trim();
    let mut written = Vec::new();

    for file in plan.included() {
        let backup_path = if backup && file.path.exists() {
            let backup_path = backup_path_for(&file.path);
            fs::copy(&file.path, &backup_path).map_err(|e| {
                GenerateError::Failed(format!(
                    "{} couldn't be backed up to {}: {e}",
                    file.file_name(),
                    backup_path.display()
                ))
            })?;
            Some(backup_path)
        } else {
            None
        };

        match file.kind {
            FileKind::Manifest => {
                let body = project
                    .manifest
                    .to_yaml()
                    .map_err(|e| GenerateError::Failed(e.friendly()))?;
                write_atomically(&file.path, &format!("{}{body}", manifest_header(project)))?;
            }
            FileKind::Desktop => {
                write_atomically(&file.path, &appdata::desktop_file(project))?;
            }
            FileKind::Metainfo => {
                write_atomically(&file.path, &appdata::metainfo(project))?;
            }
            FileKind::Icon => {
                let Some(source) = &project.icon_source else {
                    continue;
                };
                icons::install(source, &plan.folder, app_id)
                    .map_err(|err| GenerateError::Failed(err.friendly()))?;
            }
        }

        written.push(Written {
            kind: file.kind,
            path: file.path.clone(),
            backup_path,
        });
    }

    Ok(written)
}

fn backup_path_for(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push_str(".bak");
    path.with_file_name(name)
}

/// Through a temp file in the same directory, then a rename: a rename is atomic
/// on the same filesystem, so the user's file is either the old one or the new
/// one and never a truncated mixture.
fn write_atomically(path: &Path, text: &str) -> Result<(), GenerateError> {
    let temp = path.with_extension("writing");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            GenerateError::Failed(format!("{} couldn't be created: {e}", parent.display()))
        })?;
    }
    fs::write(&temp, text).map_err(|e| {
        GenerateError::Failed(format!("{} couldn't be written: {e}", temp.display()))
    })?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        GenerateError::Failed(format!("{} couldn't be written: {e}", path.display()))
    })
}

/// The comment block at the top of the manifest. A generated file that doesn't
/// say where it came from, or how to use it, is a file its owner is afraid to
/// touch.
fn manifest_header(project: &Project) -> String {
    let name = if project.name.trim().is_empty() {
        "this app".to_string()
    } else {
        project.name.clone()
    };
    let file = format!("{}.yml", project.manifest.app_id);
    format!(
        "# The recipe for building {name} as a Flatpak.\n\
         #\n\
         # Written by Pack It Flat {VERSION}. It's an ordinary text file — editing it\n\
         # by hand is fine, and opening it here again will keep whatever you change.\n\
         #\n\
         # To build it yourself:\n\
         #   flatpak-builder --force-clean --user build-dir {file}\n\
         #\n\
         # That expects the runtime named below to be installed already; this app's\n\
         # build page offers to fetch it. --install-deps-from=flathub makes\n\
         # flatpak-builder fetch it instead, but it looks for Flathub in the same\n\
         # installation as the build, so with --user it only works if you have\n\
         # Flathub as a user remote.\n\
         #\n\
         # To build and install it in one go, add --install.\n\
         \n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    fn project_in(dir: &Path) -> Project {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.Sample\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: sample\nmodules:\n  - name: sample\n",
        )
        .unwrap();
        let mut project = Project::from_import(&import, None);
        project.name = "Sample".into();
        project.summary = "Does a thing".into();
        project.license = "MIT".into();
        project.source_dir = Some(dir.to_path_buf());
        project
    }

    fn svg_bytes() -> &'static [u8] {
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"/>"
    }

    /// The failure this prevents: a Rust project builds, installs, and has a
    /// generic icon and no menu entry, because `share/` holds only the licence.
    #[test]
    fn a_hand_written_build_installs_the_apps_own_files_too() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("icon.svg");
        std::fs::write(&icon, svg_bytes()).unwrap();

        let mut project = project_in(dir.path());
        project.icon_source = Some(icon);
        // A listing the build would accept, so the metainfo is installed too.
        project.description = "A few sentences about it.".into();
        project.categories = vec!["Utility".into()];
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec![
            "cargo --offline build --release".into(),
            "install -Dm755 target/release/sample /app/bin/sample".into(),
        ];

        sync_install_commands(&mut project);
        let commands = &project.manifest.main_module().unwrap().build_commands;

        assert!(commands.iter().any(|c| c.ends_with(
            "/app/share/applications/no.oyzmo.Sample.desktop"
        )), "{commands:#?}");
        assert!(commands.iter().any(|c| c.ends_with(
            "/app/share/metainfo/no.oyzmo.Sample.metainfo.xml"
        )), "{commands:#?}");
        assert!(commands.iter().any(|c| c.ends_with(
            "/app/share/icons/hicolor/scalable/apps/no.oyzmo.Sample.svg"
        )), "{commands:#?}");
        // The build it already had is untouched, and still first.
        assert!(commands[0].starts_with("cargo"));
        assert_eq!(commands.len(), 5);

        // Running again changes nothing: the lines are matched by destination.
        sync_install_commands(&mut project);
        assert_eq!(project.manifest.main_module().unwrap().build_commands.len(), 5);
    }

    /// The failure this prevents: everything compiles, everything installs, and
    /// the build dies at the very end on `appstreamcli compose failed` because
    /// the listing has no description. An unfinished listing costs the listing,
    /// not the build.
    #[test]
    fn an_unfinished_listing_is_written_but_kept_out_of_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec!["install -Dm755 sample /app/bin/sample".into()];

        // Summary only: no description, no category.
        sync_install_commands(&mut project);
        let commands = project.manifest.main_module().unwrap().build_commands.clone();
        assert!(
            !commands.iter().any(|c| c.contains("/app/share/metainfo/")),
            "{commands:#?}"
        );
        assert!(
            commands.iter().any(|c| c.contains("/app/share/applications/")),
            "the menu entry still goes in: {commands:#?}"
        );
        // And it is still written, with the reason on its row.
        let plan = plan(&project).unwrap();
        let row = plan.files.iter().find(|f| f.kind == FileKind::Metainfo).unwrap();
        assert!(row.include);
        assert!(row.note.as_deref().is_some_and(|note| note.contains("left out of the build")));

        // Finish the listing and the line appears, without a second run adding
        // anything twice.
        project.description = "A few sentences about it.".into();
        project.categories = vec!["Utility".into()];
        sync_install_commands(&mut project);
        sync_install_commands(&mut project);
        let commands = &project.manifest.main_module().unwrap().build_commands;
        assert_eq!(
            commands.iter().filter(|c| c.contains("/app/share/metainfo/")).count(),
            1,
            "{commands:#?}"
        );

        // And taking it back out takes the line with it.
        project.description.clear();
        sync_install_commands(&mut project);
        let commands = &project.manifest.main_module().unwrap().build_commands;
        assert!(!commands.iter().any(|c| c.contains("/app/share/metainfo/")), "{commands:#?}");
    }

    /// The failure this prevents, seen on a real project: the icon is chosen
    /// after the build step was last touched, so the sync that adds install
    /// lines never runs again. The icon is written into the folder, the manifest
    /// keeps its old commands, and the finished app has a menu entry with a
    /// blank icon — `WARNING: Icon referenced in desktop file but not exported`.
    /// Writing syncs against the plan being written, which is the only moment
    /// that is reliably right.
    #[test]
    fn an_icon_chosen_after_the_build_step_still_gets_its_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec!["install -Dm755 sample /app/bin/sample".into()];

        // The state the app was in: everything synced, no icon yet.
        sync_install_commands(&mut project);
        assert!(!project
            .manifest
            .main_module()
            .unwrap()
            .build_commands
            .iter()
            .any(|c| c.contains("/app/share/icons/")));

        // Then an icon is chosen, and nothing about the build changes.
        let icon = dir.path().join("chosen.svg");
        std::fs::write(&icon, svg_bytes()).unwrap();
        project.icon_source = Some(icon);

        // What `forms::write_files` does before writing.
        let plan = plan(&project).unwrap();
        sync_install_commands_with(&mut project, &plan);

        let commands = &project.manifest.main_module().unwrap().build_commands;
        assert!(
            commands.iter().any(|c| c.ends_with(
                "/app/share/icons/hicolor/scalable/apps/no.oyzmo.Sample.svg"
            )),
            "{commands:#?}"
        );
    }

    /// The failure this prevents: a project started from a Git address builds
    /// its code from the repository, so the desktop entry, metainfo and icon
    /// this app writes into the folder are not in the build at all. It compiles
    /// everything and then stops on `install: cannot stat 'app.desktop'`. A real
    /// build of this app's own repository did exactly that.
    #[test]
    fn files_written_here_travel_with_a_build_that_fetches_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("icon.svg");
        std::fs::write(&icon, svg_bytes()).unwrap();

        let mut project = project_in(dir.path());
        project.icon_source = Some(icon);
        project.description = "A few sentences about it.".into();
        project.categories = vec!["Utility".into()];
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec!["install -Dm755 sample /app/bin/sample".into()];
        // The code comes from a repository, not from this folder.
        module.sources = vec![crate::manifest::SourceEntry::Source(crate::manifest::Source {
            kind: crate::manifest::SourceKind::Git,
            url: Some("https://example.com/sample.git".into()),
            commit: Some("f".repeat(40)),
            ..Default::default()
        })];

        sync_install_commands(&mut project);
        let module = project.manifest.main_module().unwrap();
        let carried: Vec<String> = module
            .sources
            .iter()
            .filter_map(|entry| entry.as_source())
            .filter(|source| source.kind == crate::manifest::SourceKind::File)
            .map(|source| source.path.clone().unwrap_or_default())
            .collect();

        assert!(carried.iter().any(|p| p.ends_with(".desktop")), "{carried:#?}");
        assert!(carried.iter().any(|p| p.ends_with(".metainfo.xml")), "{carried:#?}");
        let icon = module
            .sources
            .iter()
            .filter_map(|entry| entry.as_source())
            .find(|source| source.path.as_deref().is_some_and(|p| p.ends_with(".svg")))
            .expect("the icon travels too");
        // Without `dest` it would land at the top and the install line, which
        // names the folder it belongs in, would miss it.
        assert_eq!(icon.dest.as_deref(), Some("icons/hicolor/scalable/apps"));

        // Running twice adds nothing a second time.
        sync_install_commands(&mut project);
        let files = project
            .manifest
            .main_module()
            .unwrap()
            .sources
            .iter()
            .filter_map(|entry| entry.as_source())
            .filter(|source| source.kind == crate::manifest::SourceKind::File)
            .count();
        assert_eq!(files, 3, "one entry each, not two");
    }

    /// A folder source already hands the build everything written beside the
    /// manifest, so carrying the same files again would be noise.
    #[test]
    fn a_folder_build_carries_nothing_extra() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());
        project.description = "A few sentences about it.".into();
        project.categories = vec!["Utility".into()];
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec!["install -Dm755 sample /app/bin/sample".into()];
        module.sources = vec![crate::manifest::SourceEntry::Source(
            crate::manifest::Source::dir("."),
        )];

        sync_install_commands(&mut project);
        assert_eq!(project.manifest.main_module().unwrap().sources.len(), 1);
    }

    /// Unticking a file on the review step takes its install line out with it: a
    /// line installing a file that was never written fails the build on a
    /// missing file, which is a baffling way to be told what you switched off.
    #[test]
    fn a_file_switched_off_loses_its_install_line() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());
        project.description = "A few sentences about it.".into();
        project.categories = vec!["Utility".into()];
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::Simple);
        module.build_commands = vec!["install -Dm755 sample /app/bin/sample".into()];

        sync_install_commands(&mut project);
        assert!(project
            .manifest
            .main_module()
            .unwrap()
            .build_commands
            .iter()
            .any(|c| c.contains("/app/share/applications/")));

        let mut plan = plan(&project).unwrap();
        plan.set_included(FileKind::Desktop, false);
        sync_install_commands_with(&mut project, &plan);

        let commands = &project.manifest.main_module().unwrap().build_commands;
        assert!(!commands.iter().any(|c| c.contains("/app/share/applications/")), "{commands:#?}");
        assert!(commands.iter().any(|c| c.contains("/app/share/metainfo/")), "{commands:#?}");
    }

    /// CMake and Meson install through their own rules; adding commands to those
    /// modules would override the build system that already knows what to do.
    #[test]
    fn a_build_system_that_installs_for_itself_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());
        let module = project.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(crate::manifest::BuildSystem::CMakeNinja);
        module.build_commands = vec!["echo hello".into()];

        sync_install_commands(&mut project);
        assert_eq!(
            project.manifest.main_module().unwrap().build_commands,
            vec!["echo hello".to_string()]
        );
    }

    #[test]
    fn the_plan_names_every_file_after_the_app_id() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan(&project_in(dir.path())).unwrap();

        let names: Vec<String> = plan.files.iter().map(|file| file.file_name()).collect();
        assert_eq!(
            names,
            vec![
                "no.oyzmo.Sample.yml",
                "no.oyzmo.Sample.desktop",
                "no.oyzmo.Sample.metainfo.xml"
            ]
        );
        assert!(plan.files.iter().all(|file| file.include));
        assert!(!plan.replaces_anything());
        // Each one says what it is for.
        assert!(plan.files.iter().all(|file| !file.kind.purpose().is_empty()));
    }

    #[test]
    fn writing_produces_files_the_app_can_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_in(dir.path());
        let plan = plan(&project).unwrap();
        let written = write(&project, &plan, true).unwrap();

        assert_eq!(written.len(), 3);
        assert!(written.iter().all(|file| file.backup_path.is_none()));

        let manifest_text = fs::read_to_string(&plan.manifest().path).unwrap();
        let reimported = manifest::parse_str(&manifest_text).unwrap();
        assert_eq!(reimported.manifest, project.manifest);

        let desktop = fs::read_to_string(dir.path().join("no.oyzmo.Sample.desktop")).unwrap();
        assert!(desktop.contains("Icon=no.oyzmo.Sample"));

        let metainfo = fs::read_to_string(dir.path().join("no.oyzmo.Sample.metainfo.xml")).unwrap();
        assert!(metainfo.contains("<id>no.oyzmo.Sample</id>"));
    }

    #[test]
    fn an_icon_is_copied_into_the_layout_flatpak_expects() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("drawing.svg");
        fs::write(&source, svg_bytes()).unwrap();

        let mut project = project_in(dir.path());
        project.icon_source = Some(source);

        let plan = plan(&project).unwrap();
        let icon = plan
            .files
            .iter()
            .find(|file| file.kind == FileKind::Icon)
            .expect("the icon is planned");
        assert_eq!(
            icon.relative,
            Path::new("icons/hicolor/scalable/apps/no.oyzmo.Sample.svg")
        );

        write(&project, &plan, false).unwrap();
        assert!(dir.path().join(&icon.relative).is_file());
    }

    #[test]
    fn an_unusable_picture_does_not_stop_the_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes.txt");
        fs::write(&source, b"not a picture").unwrap();

        let mut project = project_in(dir.path());
        project.icon_source = Some(source);

        let plan = plan(&project).unwrap();
        let icon = plan
            .files
            .iter()
            .find(|file| file.kind == FileKind::Icon)
            .unwrap();
        assert!(!icon.include, "it is not written");
        assert!(icon.note.as_ref().unwrap().contains("SVG or a PNG"));

        let written = write(&project, &plan, false).unwrap();
        assert_eq!(written.len(), 3, "everything else is still written");
    }

    #[test]
    fn existing_files_are_backed_up_one_by_one() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_in(dir.path());
        fs::write(dir.path().join("no.oyzmo.Sample.yml"), "old manifest\n").unwrap();
        fs::write(dir.path().join("no.oyzmo.Sample.desktop"), "old entry\n").unwrap();

        let plan = plan(&project).unwrap();
        assert!(plan.replaces_anything());

        let written = write(&project, &plan, true).unwrap();
        let backed_up: Vec<&Written> = written
            .iter()
            .filter(|file| file.backup_path.is_some())
            .collect();
        assert_eq!(backed_up.len(), 2);

        assert_eq!(
            fs::read_to_string(dir.path().join("no.oyzmo.Sample.yml.bak")).unwrap(),
            "old manifest\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("no.oyzmo.Sample.desktop.bak")).unwrap(),
            "old entry\n"
        );
    }

    #[test]
    fn files_can_be_skipped_individually() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_in(dir.path());
        let mut plan = plan(&project).unwrap();
        plan.set_included(FileKind::Desktop, false);
        plan.set_included(FileKind::Metainfo, false);

        let written = write(&project, &plan, false).unwrap();
        assert_eq!(written.len(), 1);
        assert!(dir.path().join("no.oyzmo.Sample.yml").is_file());
        assert!(!dir.path().join("no.oyzmo.Sample.desktop").exists());
    }

    #[test]
    fn nothing_is_planned_without_an_app_id_or_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = project_in(dir.path());

        project.manifest.app_id.clear();
        assert!(matches!(plan(&project), Err(GenerateError::NoAppId)));

        project.manifest.app_id = "no.oyzmo.Sample".into();
        project.source_dir = None;
        assert!(matches!(plan(&project), Err(GenerateError::NoFolder)));

        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn no_leftovers_from_writing() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_in(dir.path());
        let plan = plan(&project).unwrap();
        write(&project, &plan, true).unwrap();

        let mut names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "no.oyzmo.Sample.desktop",
                "no.oyzmo.Sample.metainfo.xml",
                "no.oyzmo.Sample.yml"
            ]
        );
    }
}
