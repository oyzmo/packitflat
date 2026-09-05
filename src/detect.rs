//! Work out what kind of project a folder holds, and say so in plain English.
//!
//! The point isn't only to pre-fill fields — it's that the user is *told* what
//! was found and why, so the first screen after picking a folder reads like an
//! answer rather than a form.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    Rust,
    Meson,
    CMake,
    Node,
    Python,
    Autotools,
    Make,
    Unknown,
}

impl ProjectKind {
    /// Name a beginner recognises, not the build tool's own spelling.
    pub fn title(&self) -> &'static str {
        match self {
            ProjectKind::Rust => "Rust project",
            ProjectKind::Meson => "Meson project",
            ProjectKind::CMake => "CMake project",
            ProjectKind::Node => "Node.js project",
            ProjectKind::Python => "Python project",
            ProjectKind::Autotools => "Autotools project",
            ProjectKind::Make => "Makefile project",
            ProjectKind::Unknown => "Unrecognised project",
        }
    }

    /// The `buildsystem:` value flatpak-builder wants. Rust and Node have no
    /// build system of their own in flatpak-builder's vocabulary; their builds
    /// are spelled out as commands instead.
    pub fn buildsystem(&self) -> &'static str {
        match self {
            ProjectKind::Meson => "meson",
            ProjectKind::CMake => "cmake-ninja",
            ProjectKind::Autotools => "autotools",
            _ => "simple",
        }
    }

    /// SDK extensions this kind of project needs to compile inside the sandbox.
    pub fn sdk_extensions(&self) -> Vec<String> {
        match self {
            ProjectKind::Rust => vec!["org.freedesktop.Sdk.Extension.rust-stable".into()],
            ProjectKind::Node => vec!["org.freedesktop.Sdk.Extension.node22".into()],
            _ => Vec::new(),
        }
    }

    /// Whether the build downloads dependencies, which a Flatpak build cannot
    /// do — these are the projects that need a vendored sources file.
    pub fn needs_offline_sources(&self) -> bool {
        matches!(self, ProjectKind::Rust | ProjectKind::Node | ProjectKind::Python)
    }
}

#[derive(Debug, Clone)]
pub struct Detection {
    pub kind: ProjectKind,
    /// The file that gave it away, relative to the folder.
    pub evidence: Option<String>,
    /// A name to suggest for the app and its module.
    pub suggested_name: String,
    /// The program the build installs, read out of the project's own files when
    /// they say. This is what the app runs, so guessing it from the folder name
    /// would be worse than useless.
    pub binary: Option<String>,
    /// Build commands that would actually work, for the build systems flatpak
    /// can't drive on its own. The brief's promise is that pressing Next through
    /// a detected project gives a working build, and this is where that is kept.
    pub build_commands: Vec<String>,
}

impl Detection {
    /// The sentence shown under "Here's what I found". Says what was seen,
    /// what was concluded, and what will be done about it.
    pub fn explanation(&self) -> String {
        match (&self.evidence, self.kind) {
            (Some(file), ProjectKind::Rust) => format!(
                "Found {file}, so this is a Rust project. It will be built with cargo, \
                 and the Rust compiler will be added to the build environment. Rust \
                 downloads its dependencies, which a Flatpak build can't do, so the \
                 list of them has to be prepared in advance."
            ),
            (Some(file), ProjectKind::Node) => format!(
                "Found {file}, so this is a Node.js project. Node will be added to the \
                 build environment. Its dependencies have to be prepared in advance, \
                 because a Flatpak build has no internet access."
            ),
            (Some(file), ProjectKind::Python) => format!(
                "Found {file}, so this is a Python project. Its dependencies have to be \
                 prepared in advance, because a Flatpak build has no internet access."
            ),
            (Some(file), ProjectKind::Meson) => format!(
                "Found {file}, so this project is built with Meson. Flatpak knows how to \
                 build Meson projects on its own — you shouldn't need to write any commands."
            ),
            (Some(file), ProjectKind::CMake) => format!(
                "Found {file}, so this project is built with CMake. Flatpak knows how to \
                 build CMake projects on its own."
            ),
            (Some(file), ProjectKind::Autotools) => format!(
                "Found {file}, so this project uses the classic configure-and-make setup. \
                 Flatpak knows how to build those on its own."
            ),
            (Some(file), ProjectKind::Make) => format!(
                "Found {file}. There's a Makefile but no configure script, so the build \
                 commands will be written out plainly and you can adjust them."
            ),
            _ => "Nothing in this folder identifies how the project is built, so the \
                  build commands are left for you to fill in. That's normal for a project \
                  that just wraps an existing program."
                .to_string(),
        }
    }
}

/// Markers in priority order. Meson and CMake come before Makefile: a project
/// using either usually has a generated Makefile lying around too, and matching
/// that first would suggest the wrong build system.
const MARKERS: &[(&str, ProjectKind)] = &[
    ("Cargo.toml", ProjectKind::Rust),
    ("meson.build", ProjectKind::Meson),
    ("CMakeLists.txt", ProjectKind::CMake),
    ("package.json", ProjectKind::Node),
    ("pyproject.toml", ProjectKind::Python),
    ("setup.py", ProjectKind::Python),
    ("configure.ac", ProjectKind::Autotools),
    ("configure", ProjectKind::Autotools),
    ("Makefile", ProjectKind::Make),
];

pub fn detect(dir: &Path) -> Detection {
    let (kind, evidence) = MARKERS
        .iter()
        .find(|(file, _)| dir.join(file).is_file())
        .map(|(file, kind)| (*kind, Some((*file).to_string())))
        .unwrap_or((ProjectKind::Unknown, None));

    let suggested_name = suggest_name(dir);
    let binary = binary_name(dir, kind);
    let program = binary.clone().unwrap_or_else(|| suggested_name.clone());

    Detection {
        kind,
        evidence,
        build_commands: build_commands(kind, &program),
        binary,
        suggested_name,
    }
}

/// What the built program is called, according to the project itself. Cargo and
/// npm both record it; nothing else here does, and inventing one would put a
/// wrong `command:` in the manifest, which fails only at the moment someone
/// tries to launch the finished app.
fn binary_name(dir: &Path, kind: ProjectKind) -> Option<String> {
    match kind {
        ProjectKind::Rust => cargo_package_name(&std::fs::read_to_string(dir.join("Cargo.toml")).ok()?),
        ProjectKind::Node => {
            // JSON is YAML, so the parser already in the app reads package.json.
            let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
            let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).ok()?;
            value.get("name")?.as_str().map(str::to_string)
        }
        _ => None,
    }
}

/// The `name` of the `[package]` table, without a TOML parser. Only the package
/// table counts: `[dependencies]` and `[[bin]]` have `name` keys too, and taking
/// the first one in the file would pick a dependency's.
fn cargo_package_name(text: &str) -> Option<String> {
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        if let Some(value) = line.strip_prefix("name") {
            let value = value.trim_start().strip_prefix('=')?.trim();
            let value = value.trim_matches(|c| c == '"' || c == '\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Commands for the "simple" build systems — the ones flatpak-builder cannot
/// drive itself. Meson, CMake and autotools get none, because giving them any
/// would override the build system that already knows what to do.
fn build_commands(kind: ProjectKind, program: &str) -> Vec<String> {
    match kind {
        ProjectKind::Rust => vec![
            // --offline because a Flatpak build has no network: the crates come
            // from the vendored sources list instead.
            "cargo --offline build --release".to_string(),
            format!("install -Dm755 target/release/{program} /app/bin/{program}"),
        ],
        ProjectKind::Python => vec![
            "pip3 install --no-index --find-links=\"file://${PWD}\" --prefix=${FLATPAK_DEST} ."
                .to_string(),
        ],
        ProjectKind::Make => vec![
            "make".to_string(),
            "make install PREFIX=/app".to_string(),
        ],
        _ => Vec::new(),
    }
}

/// The folder's own name, cleaned up into something usable as a module name.
fn suggest_name(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("app")
        .to_lowercase();
    let cleaned: String = raw
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

/// Whether the project's own build files say where the finished program goes.
///
/// flatpak-builder always runs the install step, so a project that never
/// installs anything compiles perfectly and then dies on the last line with
/// `ninja: error: unknown target 'install'`. Reading the answer out of the build
/// files beforehand is the only way to say so before the build.
///
/// `None` means "can't tell from here": either this isn't a kind of project
/// whose build files declare it, or the marker file isn't there to read.
/// Only CMake and Meson are answered — both are declarative enough to read, and
/// both are driven by flatpak-builder itself. A makefile can hide an install
/// target behind an include or a recursive `make`, and guessing wrong there
/// would mean warning about a build that works.
/// The application ID the program asks the session bus for, if it says so in a
/// way that can be read without running it.
///
/// **A Flatpak may only own its own app ID.** A GTK app registers its
/// application ID on the session bus at startup, and the sandbox refuses any
/// other name — so an app whose code says `com.example.Thing` inside a Flatpak
/// called `no.oyzmo.Thing` installs perfectly, appears in the menu, and then
/// dies on launch with
/// `Failed to register: GDBus.Error:org.freedesktop.DBus.Error.ServiceUnknown`,
/// which names neither the ID nor the manifest. (Seen on a real app.)
///
/// Read out of the source rather than guessed: an `application_id("…")` call or
/// a `const APP_ID = "…"` line, in Rust, Python, JavaScript or C alike, since
/// they all spell it much the same way. `None` means nothing was found and
/// nothing will be said — a false alarm here would send someone editing code
/// that was already right.
pub fn declared_application_id(dir: &Path) -> Option<String> {
    let mut found = None;
    for text in source_files(dir, 3) {
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("//") || line.starts_with('#') || line.starts_with('*') {
                continue;
            }
            if !line.contains("application_id")
                && !line.contains("APP_ID")
                && !line.contains("applicationId")
            {
                continue;
            }
            if let Some(id) = quoted_app_id(line) {
                // The first one wins, and a second disagreeing one means the
                // file is not saying anything simple enough to act on.
                match &found {
                    None => found = Some(id),
                    Some(first) if *first != id => return None,
                    Some(_) => {}
                }
            }
        }
    }
    found
}

/// A quoted reverse-DNS name from a line, if there is exactly one.
fn quoted_app_id(line: &str) -> Option<String> {
    let mut ids = line
        .split(['"', '\''])
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|value| {
            value.split('.').count() >= 3
                && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
                && !value.ends_with(".rs")
                && !value.ends_with(".py")
                && !value.ends_with(".ui")
        });
    let first = ids.next()?;
    if ids.next().is_some() {
        return None;
    }
    Some(first.to_string())
}

/// Source files worth reading, a few levels down, skipping the places a build
/// leaves copies of everything.
fn source_files(dir: &Path, depth: usize) -> Vec<String> {
    let mut out = Vec::new();
    collect_sources(dir, depth, &mut out);
    out
}

fn collect_sources(dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if depth == 0
                || name.starts_with('.')
                || matches!(
                    name.as_str(),
                    "target" | "build" | "dist" | "node_modules" | "vendor" | "build-dir"
                )
            {
                continue;
            }
            collect_sources(&path, depth - 1, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "py" | "js" | "c" | "cpp" | "vala")
        ) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.push(text);
            }
        }
    }
}

pub fn declares_install(dir: &Path, kind: ProjectKind) -> Option<bool> {
    // The needles err towards silence on purpose: finding one means no warning,
    // so a loose match costs nothing and a missed one would cost a false alarm.
    let (marker, needle) = match kind {
        ProjectKind::CMake => ("CMakeLists.txt", "install("),
        ProjectKind::Meson => ("meson.build", "install"),
        _ => return None,
    };
    if !dir.join(marker).is_file() {
        return None;
    }

    Some(build_files(dir, marker, 3).iter().any(|text| mentions(text, needle)))
}

/// Every copy of a build file in the project, not just the top one: an
/// `install()` two directories down in an `add_subdirectory()` still installs
/// the program, and warning about it would be wrong.
fn build_files(dir: &Path, marker: &str, depth: u32) -> Vec<String> {
    /// Generated output and other people's code. Scanning a `build/` directory
    /// finds CMake's own generated files, which mention anything at all.
    const NOISE: &[&str] = &[
        "build",
        "builddir",
        "build-dir",
        "_build",
        "_deps",
        "subprojects",
        "target",
        "node_modules",
        ".flatpak-builder",
        ".git",
    ];

    let mut found = Vec::new();
    if let Ok(text) = std::fs::read_to_string(dir.join(marker)) {
        found.push(text);
    }
    if depth == 0 {
        return found;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') || NOISE.contains(&name.as_str()) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            found.extend(build_files(&entry.path(), marker, depth - 1));
        }
    }
    found
}

/// The needle as a word, so `uninstall(` isn't read as `install(`. CMake's
/// commands are case-insensitive, so the comparison is too.
fn mentions(text: &str, needle: &str) -> bool {
    let lower = text.to_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find(needle) {
        let at = from + at;
        let before = lower[..at].chars().next_back();
        if !before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return true;
        }
        from = at + needle.len();
    }
    false
}

/// Folders worth offering as "recently used" or as a starting point. Kept here
/// because the welcome page needs it and it is pure path logic.
pub fn looks_like_a_project(dir: &Path) -> bool {
    detect(dir).kind != ProjectKind::Unknown
}

/// Absolute, symlink-free where possible — paths go into the manifest, and a
/// relative one there means a build that only works from one directory.
pub fn canonical(dir: &Path) -> PathBuf {
    dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            std::fs::write(dir.path().join(f), "").unwrap();
        }
        dir
    }

    #[test]
    fn finds_rust() {
        let dir = dir_with(&["Cargo.toml", "Makefile"]);
        let d = detect(dir.path());
        assert_eq!(d.kind, ProjectKind::Rust);
        assert_eq!(d.evidence.as_deref(), Some("Cargo.toml"));
        assert!(d.kind.needs_offline_sources());
        assert!(d.explanation().contains("Cargo.toml"));
    }

    #[test]
    fn a_detected_rust_project_arrives_ready_to_build() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"cleaner\"\nversion = \"1.0.0\"\n\n\
             [dependencies]\nname_lookalike = \"1\"\n",
        )
        .unwrap();

        let d = detect(dir.path());
        assert_eq!(d.binary.as_deref(), Some("cleaner"));
        assert_eq!(d.build_commands.len(), 2);
        assert!(d.build_commands[1].contains("/app/bin/cleaner"));
        // The crates come from the vendored list, so the build must not reach out.
        assert!(d.build_commands[0].contains("--offline"));
    }

    #[test]
    fn the_package_name_comes_from_the_package_table_only() {
        assert_eq!(
            cargo_package_name("[dependencies]\nname = \"wrong\"\n\n[package]\nname = \"right\"\n")
                .as_deref(),
            Some("right")
        );
        assert_eq!(cargo_package_name("[workspace]\nmembers = []\n"), None);
        assert_eq!(cargo_package_name(""), None);
    }

    #[test]
    fn node_projects_name_themselves_in_package_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "note-taker", "version": "2.0.0"}"#,
        )
        .unwrap();
        let d = detect(dir.path());
        assert_eq!(d.kind, ProjectKind::Node);
        assert_eq!(d.binary.as_deref(), Some("note-taker"));
        // Node builds vary too much to guess commands for.
        assert!(d.build_commands.is_empty());
    }

    #[test]
    fn build_systems_flatpak_drives_itself_get_no_commands() {
        let dir = dir_with(&["meson.build"]);
        assert!(detect(dir.path()).build_commands.is_empty());
    }

    #[test]
    fn meson_wins_over_a_generated_makefile() {
        let dir = dir_with(&["Makefile", "meson.build"]);
        assert_eq!(detect(dir.path()).kind, ProjectKind::Meson);
        assert_eq!(detect(dir.path()).kind.buildsystem(), "meson");
    }

    #[test]
    fn a_cmake_project_is_asked_whether_it_installs_anything() {
        let dir = tempfile::tempdir().unwrap();
        let list = dir.path().join("CMakeLists.txt");

        std::fs::write(&list, "add_executable(app main.cpp)\n").unwrap();
        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), Some(false));

        std::fs::write(&list, "add_executable(app main.cpp)\nINSTALL(TARGETS app)\n").unwrap();
        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), Some(true));
    }

    /// A custom `uninstall` target is not an install rule, and the build fails
    /// just the same — reading it as one would hide the warning.
    #[test]
    fn uninstall_is_not_read_as_install() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CMakeLists.txt"),
            "add_custom_target(uninstall(fake))\n",
        )
        .unwrap();
        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), Some(false));
    }

    #[test]
    fn an_install_rule_in_a_subdirectory_counts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "add_subdirectory(src)\n").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/CMakeLists.txt"), "install(TARGETS app)\n").unwrap();

        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), Some(true));
    }

    /// CMake's own generated files under `build/` mention everything; scanning
    /// them would answer "yes" for every project that has ever been built.
    #[test]
    fn generated_output_is_not_scanned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "add_executable(app a.c)\n").unwrap();
        std::fs::create_dir(dir.path().join("build")).unwrap();
        std::fs::write(dir.path().join("build/CMakeLists.txt"), "install(FILES x)\n").unwrap();

        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), Some(false));
    }

    /// A makefile can hide its install target behind an include or a recursive
    /// make, so the honest answer is "can't tell" rather than a false alarm.
    #[test]
    fn kinds_we_cannot_read_an_answer_from_say_so() {
        let dir = dir_with(&["Makefile"]);
        assert_eq!(declares_install(dir.path(), ProjectKind::Make), None);
        assert_eq!(declares_install(dir.path(), ProjectKind::Rust), None);
        // CMake claimed, but no CMakeLists.txt to read.
        assert_eq!(declares_install(dir.path(), ProjectKind::CMake), None);
    }

    #[test]
    fn meson_is_read_for_its_install_keyword() {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("meson.build");

        std::fs::write(&build, "executable('app', 'main.c')\n").unwrap();
        assert_eq!(declares_install(dir.path(), ProjectKind::Meson), Some(false));

        std::fs::write(&build, "executable('app', 'main.c', install : true)\n").unwrap();
        assert_eq!(declares_install(dir.path(), ProjectKind::Meson), Some(true));
    }

    #[test]
    fn empty_folder_is_unknown_but_still_explained() {
        let dir = dir_with(&[]);
        let d = detect(dir.path());
        assert_eq!(d.kind, ProjectKind::Unknown);
        assert!(!d.explanation().is_empty());
        assert!(!looks_like_a_project(dir.path()));
    }

    #[test]
    fn suggested_name_is_usable_as_a_module_name() {
        let dir = tempfile::tempdir().unwrap();
        let messy = dir.path().join("My Cool App!");
        std::fs::create_dir(&messy).unwrap();
        assert_eq!(detect(&messy).suggested_name, "my-cool-app");
    }
}
