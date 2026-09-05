//! The whole path the guided steps walk, without a display: point at a folder,
//! fill in what the seven steps ask for, and write files that read back as the
//! same project.
//!
//! The GUI is a view over exactly these calls, so if this passes, the only thing
//! left to get wrong upstairs is which widget is wired to which field.

use std::fs;

use packitflat::generate::FileKind;
use packitflat::manifest::{BuildSystem, SourceKind};
use packitflat::permissions::{Key, Permissions};
use packitflat::project::Project;
use packitflat::validate::{Issues, Severity};
use packitflat::{generate, manifest, oars, runtimes, spdx, validate};

fn rust_project_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("metadata-cleaner");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("Cargo.toml"), "[package]\nname = \"cleaner\"\n").unwrap();
    fs::write(
        project.join("icon.svg"),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"/>",
    )
    .unwrap();
    dir
}

/// The brief's promise: on a project the app recognises, the only thing the user
/// has to decide is the app ID, because that is the one answer no amount of
/// scanning can produce.
#[test]
fn a_detected_project_only_needs_an_app_id() {
    let temp = rust_project_folder();
    let (mut project, _) = Project::from_folder(&temp.path().join("metadata-cleaner"));

    let blocking: Vec<_> = validate::project(&project)
        .into_iter()
        .filter(|issue| issue.severity == Severity::Error)
        .collect();
    assert_eq!(blocking.len(), 1, "{blocking:#?}");
    assert_eq!(blocking[0].field, validate::Field::AppId);

    // It found the program's real name, not the folder's.
    assert_eq!(project.manifest.command, "cleaner");

    project.manifest.app_id = "no.oyzmo.Cleaner".into();
    assert_eq!(validate::project(&project).errors(), 0);

    // …and that manifest is one flatpak-builder could actually run.
    let plan = generate::plan(&project).unwrap();
    generate::write(&project, &plan, false).unwrap();
    let text = fs::read_to_string(&plan.manifest().path).unwrap();
    assert!(text.contains("cargo --offline build --release"), "{text}");
    assert!(text.contains("install -Dm755 target/release/cleaner"), "{text}");

    // And the build can find the compiler it just asked for. Without this line
    // the build downloads every crate and then stops with "cargo: command not
    // found" — which is what a real build did before this was checked.
    assert!(
        text.contains("append-path: /usr/lib/sdk/rust-stable/bin"),
        "the Rust extension is switched on but nothing puts it on the path:\n{text}"
    );
    assert!(validate::project(&project)
        .iter()
        .all(|issue| !issue.message.contains("where they are")));
}

#[test]
fn a_folder_becomes_a_finished_set_of_files() {
    let temp = rust_project_folder();
    let folder = temp.path().join("metadata-cleaner");

    // Step 0: pointing at the folder. Detection fills in what it can.
    let (mut project, detection) = Project::from_folder(&folder);
    assert_eq!(detection.kind.title(), "Rust project");
    assert!(project
        .manifest
        .sdk_extensions
        .iter()
        .any(|e| e.contains("rust-stable")));

    // Step 1: the basics.
    project.name = "Metadata Cleaner".into();
    project.manifest.app_id = "no.oyzmo.MetadataCleaner".into();
    project.summary = "Strip metadata from files".into();
    project.description = "Removes Exif, GPS and document properties.".into();
    project.license = "GPL-3.0-or-later".into();
    project.developer = "oyzmo".into();
    project.homepage = "http://oyzmo.no".into();
    assert!(spdx::is_known(&project.license));

    // Step 2: runtime and SDK, taken from the picker rather than typed.
    let choices = runtimes::catalogue(&[], &[]);
    let chosen = &choices[0];
    assert_eq!(chosen.support, runtimes::Support::Current);
    project.manifest.runtime = chosen.runtime.clone();
    project.manifest.runtime_version = chosen.version.clone();
    project.manifest.sdk = chosen.sdk.clone();

    // Step 3: where the code is. from_folder already added the folder itself.
    let module = project.manifest.main_module().unwrap();
    assert_eq!(module.sources.len(), 1);
    assert_eq!(module.sources[0].as_source().unwrap().kind, SourceKind::Dir);

    // Step 4: how it is built.
    project.manifest.command = "cleaner".into();
    let module = project.manifest.main_module_mut().unwrap();
    module.buildsystem = Some(BuildSystem::Simple);
    module.build_commands = vec!["cargo --offline build --release".into()];

    // Step 5: what it may do. The starter set draws a window and nothing else.
    let mut permissions = Permissions::parse(&project.manifest.finish_args);
    assert!(permissions.risks().is_empty(), "{:#?}", permissions.risks());
    permissions.set(Key::Network, true);
    project.manifest.finish_args = permissions.to_args();
    assert!(validate::project(&project)
        .iter()
        .any(|issue| issue.field == validate::Field::Permissions));

    // Step 6: how it looks.
    project.categories = vec!["Utility".into()];
    project.keywords = vec!["metadata".into(), "privacy".into()];
    project.icon_source = Some(folder.join("icon.svg"));
    project.release_version = "1.0.0".into();
    project.release_date = "2026-09-04".into();
    project.release_notes = "First release.".into();
    oars::set_answer(&mut project.content_rating, "social", 1);

    // Step 7: nothing blocking left, so everything can be written.
    let issues = validate::project(&project);
    assert_eq!(issues.errors(), 0, "{issues:#?}");
    assert_eq!(
        issues.warnings(),
        1,
        "only the network permission is worth mentioning: {issues:#?}"
    );

    let plan = generate::plan(&project).unwrap();
    let names: Vec<String> = plan.files.iter().map(|file| file.file_name()).collect();
    assert_eq!(
        names,
        vec![
            "no.oyzmo.MetadataCleaner.yml",
            "no.oyzmo.MetadataCleaner.desktop",
            "no.oyzmo.MetadataCleaner.metainfo.xml",
            "no.oyzmo.MetadataCleaner.svg",
        ]
    );
    let written = generate::write(&project, &plan, true).unwrap();
    assert_eq!(written.len(), 4);
    assert!(written.iter().all(|file| file.backup_path.is_none()));

    // The manifest reads back as the same project.
    let text = fs::read_to_string(&plan.manifest().path).unwrap();
    let reimported = manifest::parse_str(&text).unwrap();
    assert!(
        reimported.report.is_empty(),
        "the app should understand everything it writes: {:#?}",
        reimported.report
    );
    assert_eq!(reimported.manifest, project.manifest);
    assert!(text.contains("--share=network"));

    // The desktop entry and the metainfo agree with it, name for name.
    let desktop = fs::read_to_string(folder.join("no.oyzmo.MetadataCleaner.desktop")).unwrap();
    assert!(desktop.contains("Icon=no.oyzmo.MetadataCleaner\n"));
    assert!(desktop.contains("Exec=cleaner\n"));
    assert!(desktop.contains("Categories=Utility;\n"));

    let metainfo = fs::read_to_string(folder.join("no.oyzmo.MetadataCleaner.metainfo.xml")).unwrap();
    assert!(metainfo.contains("<id>no.oyzmo.MetadataCleaner</id>"));
    assert!(metainfo
        .contains("<launchable type=\"desktop-id\">no.oyzmo.MetadataCleaner.desktop</launchable>"));
    assert!(metainfo.contains("<content_attribute id=\"social-chat\">intense</content_attribute>"));
    assert!(metainfo.contains("<release version=\"1.0.0\" date=\"2026-09-04\">"));

    // And the icon is where Flatpak looks for it, named after the app ID.
    assert!(folder
        .join("icons/hicolor/scalable/apps/no.oyzmo.MetadataCleaner.svg")
        .is_file());
}

/// The desktop entry and the metainfo are written by hand here, so they are
/// checked against the tools that judge them for real. Skipped where those tools
/// aren't installed rather than faked.
#[test]
fn the_generated_files_pass_the_system_validators() {
    let temp = rust_project_folder();
    let folder = temp.path().join("metadata-cleaner");
    let (mut project, _) = Project::from_folder(&folder);

    project.name = "Metadata Cleaner".into();
    project.manifest.app_id = "no.oyzmo.MetadataCleaner".into();
    project.manifest.command = "cleaner".into();
    project.summary = "Strip metadata from files".into();
    project.description = "Removes Exif, GPS and document properties.\n\nWorks offline.".into();
    project.license = "GPL-3.0-or-later".into();
    project.developer = "oyzmo".into();
    project.homepage = "http://oyzmo.no".into();
    project.categories = vec!["Utility".into()];
    project.keywords = vec!["metadata".into()];
    project.icon_source = Some(folder.join("icon.svg"));
    project.release_version = "1.0.0".into();
    project.release_date = "2026-09-04".into();
    project.release_notes = "First release.".into();

    let plan = generate::plan(&project).unwrap();
    generate::write(&project, &plan, false).unwrap();

    check_with(
        "desktop-file-validate",
        &[folder.join("no.oyzmo.MetadataCleaner.desktop")],
    );
    check_with(
        "appstreamcli",
        &[
            std::path::PathBuf::from("validate"),
            std::path::PathBuf::from("--no-net"),
            folder.join("no.oyzmo.MetadataCleaner.metainfo.xml"),
        ],
    );
}

fn check_with(tool: &str, args: &[std::path::PathBuf]) {
    let Ok(output) = std::process::Command::new(tool).args(args).output() else {
        eprintln!("{tool} isn't installed; skipping that check");
        return;
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{tool} rejected the generated file:\n{stdout}\n{stderr}"
    );
    // appstreamcli reports errors while still exiting 0 in some versions.
    assert!(
        !stdout.contains("E: ") && !stderr.contains("E: "),
        "{tool} found errors:\n{stdout}\n{stderr}"
    );
}

#[test]
fn writing_twice_keeps_the_previous_files() {
    let temp = rust_project_folder();
    let folder = temp.path().join("metadata-cleaner");
    let (mut project, _) = Project::from_folder(&folder);
    project.manifest.app_id = "no.oyzmo.Twice".into();
    project.manifest.command = "twice".into();

    let plan = generate::plan(&project).unwrap();
    generate::write(&project, &plan, true).unwrap();

    project.summary = "Second time round".into();
    let plan = generate::plan(&project).unwrap();
    assert!(plan.replaces_anything());
    let written = generate::write(&project, &plan, true).unwrap();

    let manifest_backup = written
        .iter()
        .find(|file| file.kind == FileKind::Manifest)
        .and_then(|file| file.backup_path.clone())
        .expect("the old manifest was kept");
    assert!(fs::read_to_string(manifest_backup)
        .unwrap()
        .contains("no.oyzmo.Twice"));
}

#[test]
fn files_can_be_left_alone_one_at_a_time() {
    let temp = rust_project_folder();
    let folder = temp.path().join("metadata-cleaner");
    let (mut project, _) = Project::from_folder(&folder);
    project.manifest.app_id = "no.oyzmo.Picky".into();

    let mut plan = generate::plan(&project).unwrap();
    plan.set_included(FileKind::Desktop, false);
    plan.set_included(FileKind::Metainfo, false);
    let written = generate::write(&project, &plan, false).unwrap();

    assert_eq!(written.len(), 1);
    assert!(folder.join("no.oyzmo.Picky.yml").is_file());
    assert!(!folder.join("no.oyzmo.Picky.desktop").exists());
}

#[test]
fn an_imported_manifest_can_be_edited_and_written_back() {
    let temp = tempfile::tempdir().unwrap();
    let original = temp.path().join("no.oyzmo.Imported.yml");
    fs::write(
        &original,
        "app-id: no.oyzmo.Imported\n\
         runtime: org.gnome.Platform\n\
         runtime-version: '50'\n\
         sdk: org.gnome.Sdk\n\
         command: imported\n\
         cleanup:\n  - /include\n\
         finish-args:\n\
         \x20 - --socket=wayland\n\
         \x20 - --filesystem=host\n\
         \x20 - --persist=.imported\n\
         modules:\n\
         \x20 - shared-modules/glew/glew.json\n\
         \x20 - name: imported\n\
         \x20   buildsystem: meson\n\
         \x20   sources:\n\
         \x20     - type: dir\n\
         \x20       path: .\n",
    )
    .unwrap();

    let import = manifest::parse_str(&fs::read_to_string(&original).unwrap()).unwrap();
    let mut project = Project::from_import(&import, Some(&original));
    project.name = "Imported".into();
    project.summary = "Was written by hand".into();
    project.license = "MIT".into();
    project.developer = "someone".into();

    // The dangerous permission it arrived with is reported, and not blocking.
    let issues = validate::project(&project);
    assert_eq!(issues.errors(), 0);
    let permission = issues
        .iter()
        .find(|issue| issue.field == validate::Field::Permissions)
        .expect("--filesystem=host is mentioned");
    assert!(permission.message.contains("every file on the computer"));

    // Taking it away leaves everything else in the file untouched.
    let mut permissions = Permissions::parse(&project.manifest.finish_args);
    permissions.remove_filesystem(0);
    project.manifest.finish_args = permissions.to_args();
    project.manifest.runtime_version = "49".into();

    let plan = generate::plan(&project).unwrap();
    let written = generate::write(&project, &plan, true).unwrap();
    let text = fs::read_to_string(&plan.manifest().path).unwrap();

    assert!(text.contains("- shared-modules/glew/glew.json"), "{text}");
    assert!(text.contains("cleanup:"), "{text}");
    assert!(text.contains("--persist=.imported"), "{text}");
    assert!(!text.contains("--filesystem=host"), "{text}");
    assert!(text.contains("runtime-version: '49'"), "{text}");
    assert!(written
        .iter()
        .any(|file| file.kind == FileKind::Manifest && file.backup_path.is_some()));
}

#[test]
fn the_review_step_can_always_explain_what_is_missing() {
    let temp = rust_project_folder();
    let folder = temp.path().join("metadata-cleaner");
    let (project, _) = Project::from_folder(&folder);

    let issues = validate::project(&project);
    assert!(issues.errors() > 0);

    // Every issue names a step to jump to, and says both what and how.
    for issue in &issues {
        assert!(issue.field.step() < 7, "{issue:#?}");
        assert!(!issue.message.is_empty());
        assert!(!issue.fix.is_empty(), "{issue:#?}");
        assert!(
            issue.message.ends_with('.') || issue.message.ends_with('?'),
            "issue messages are sentences: {issue:#?}"
        );
    }
    assert_eq!(
        issues.worst_for_step(0),
        Some(Severity::Error),
        "the app ID is missing, and that is a step 1 problem"
    );
    // The appearance step has something to say about a project with no icon.
    assert!(issues
        .iter()
        .any(|issue| issue.field == validate::Field::Icon));
}
