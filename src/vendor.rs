//! Preparing the things a build downloads, before the build that can't.
//!
//! A Flatpak build has no network. That is the single most confusing thing about
//! packaging a modern project: `cargo build` works in a terminal and fails
//! inside the sandbox, for a reason nobody guesses on their own. The fix is
//! always the same shape — write down every dependency, with its address and its
//! checksum, in a file the manifest points at, so the build fetches nothing.
//!
//! For Rust this app does it itself: every crate's address and checksum is
//! already in `Cargo.lock`, so the list is a transcription and needs no network,
//! no Python, and no tools the user hasn't got. For Node and Python it isn't —
//! their lock files don't carry everything needed — so those get the exact
//! command to run and the file is wired in once it exists.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::detect::ProjectKind;
use crate::manifest::{BuildOptions, Manifest, Module, SourceEntry};

#[derive(Debug, Error)]
pub enum VendorError {
    #[error("{0} couldn't be read.")]
    Unreadable(String),
    #[error("{0}")]
    Unsupported(String),
}

impl VendorError {
    pub fn friendly(&self) -> String {
        self.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    Cargo,
    Node,
    Python,
}

impl Ecosystem {
    pub fn label(&self) -> &'static str {
        match self {
            Ecosystem::Cargo => "Rust crates",
            Ecosystem::Node => "Node packages",
            Ecosystem::Python => "Python packages",
        }
    }

    /// The file the list is written to, by convention.
    pub fn sources_file(&self) -> &'static str {
        match self {
            Ecosystem::Cargo => "cargo-sources.json",
            Ecosystem::Node => "node-sources.json",
            Ecosystem::Python => "python-sources.json",
        }
    }

    /// The file this app reads to work the list out, or that the tool reads.
    pub fn lock_file(&self) -> &'static str {
        match self {
            Ecosystem::Cargo => "Cargo.lock",
            Ecosystem::Node => "package-lock.json",
            Ecosystem::Python => "requirements.txt",
        }
    }

    /// Whether this app can produce the list on its own.
    pub fn can_generate_here(&self) -> bool {
        matches!(self, Ecosystem::Cargo)
    }

    /// The command that produces it when this app can't. These tools come from
    /// flatpak-builder-tools, which isn't packaged everywhere — hence the link
    /// in the explanation rather than a button that might do nothing.
    pub fn external_command(&self) -> Option<String> {
        match self {
            Ecosystem::Cargo => None,
            Ecosystem::Node => Some(
                "flatpak-node-generator npm package-lock.json -o node-sources.json".into(),
            ),
            Ecosystem::Python => Some(
                "req2flatpak --requirements-file requirements.txt --outfile python-sources.json"
                    .into(),
            ),
        }
    }

    /// What to do when the lock file isn't there — which is a different answer
    /// for each of them. Cargo writes `Cargo.lock` when the project is built,
    /// npm writes `package-lock.json` when it installs, and `requirements.txt`
    /// is written by a person: telling a Python user to "build the project once"
    /// would leave them waiting for a file nothing is going to produce.
    pub fn missing_lock_advice(&self) -> &'static str {
        match self {
            Ecosystem::Cargo => {
                "No Cargo.lock in the project folder yet. Build the project once outside \
                 Flatpak — cargo writes it — and it will appear."
            }
            Ecosystem::Node => {
                "No package-lock.json in the project folder yet. Run npm install once \
                 outside Flatpak and it will appear."
            }
            Ecosystem::Python => {
                "No requirements.txt in the project folder yet. It is a list of the \
                 packages your app needs, one per line, and it is written by hand — \
                 “pip freeze” prints a starting point."
            }
        }
    }

    /// Whether this app can write the list itself, with no tool and no network.
    ///
    /// Rust's answer is already in `Cargo.lock` — every crate's address and
    /// checksum — so it is a transcription. Node and Python are not: their lock
    /// files don't carry enough, and the list has to come from a tool this app
    /// hasn't got. The difference decides how hard the app leans on the user:
    /// something it can fix in one press is worth blocking on, something it
    /// cannot is not.
    pub fn prepared_here(&self) -> bool {
        matches!(self, Ecosystem::Cargo)
    }

    pub fn explanation(&self) -> &'static str {
        match self {
            Ecosystem::Cargo => {
                "Rust downloads its crates while building, and the build has no internet. \
                 Every crate's address and checksum is already in Cargo.lock, so this app \
                 can write the list itself — no tools, no network, nothing to install."
            }
            Ecosystem::Node => {
                "Node downloads its packages while building, and the build has no \
                 internet. The list has to be prepared by flatpak-node-generator, which \
                 comes with flatpak-builder-tools: package-lock.json alone doesn't carry \
                 everything the build needs."
            }
            Ecosystem::Python => {
                "pip downloads packages while building, and the build has no internet. \
                 req2flatpak looks each one up and writes the list; it needs the network \
                 once, now, so the build needs none later."
            }
        }
    }
}

/// Something this project will need before it can build offline.
#[derive(Debug, Clone)]
pub struct Need {
    pub ecosystem: Ecosystem,
    /// The lock file, if it is there.
    pub lock_path: Option<PathBuf>,
    /// The sources file, if it has been made already.
    pub sources_path: Option<PathBuf>,
    /// Whether the manifest already points at the sources file.
    pub wired_in: bool,
}

impl Need {
    /// What the step should say about this one.
    pub fn state(&self) -> &'static str {
        if self.sources_path.is_some() && self.wired_in {
            "Ready: the build will find everything without downloading anything."
        } else if self.sources_path.is_some() {
            "The list exists, but the manifest doesn't point at it yet."
        } else if self.lock_path.is_some() {
            "Not prepared yet. Without this the build stops as soon as it tries to \
             download something."
        } else {
            // Named rather than described. This used to be a vague "no lock file
            // found", with the pane adding a second row underneath naming the
            // file and saying what to do — two rows saying one thing, the vaguer
            // one first.
            self.ecosystem.missing_lock_advice()
        }
    }

    pub fn is_ready(&self) -> bool {
        self.sources_path.is_some() && self.wired_in
    }
}

/// What this project needs, judged from the folder and the manifest.
pub fn needs(kind: ProjectKind, folder: Option<&Path>, manifest: &Manifest) -> Vec<Need> {
    let ecosystems: &[Ecosystem] = match kind {
        ProjectKind::Rust => &[Ecosystem::Cargo],
        ProjectKind::Node => &[Ecosystem::Node],
        ProjectKind::Python => &[Ecosystem::Python],
        _ => &[],
    };

    ecosystems
        .iter()
        .map(|ecosystem| {
            let lock_path = folder
                .map(|folder| folder.join(ecosystem.lock_file()))
                .filter(|path| path.is_file());
            let sources_path = folder
                .map(|folder| folder.join(ecosystem.sources_file()))
                .filter(|path| path.is_file());

            Need {
                ecosystem: *ecosystem,
                lock_path,
                sources_path,
                wired_in: is_wired(manifest, ecosystem.sources_file()),
            }
        })
        .collect()
}

/// Whether any module already includes that file as a source.
pub fn is_wired(manifest: &Manifest, file: &str) -> bool {
    manifest.modules.iter().any(|entry| {
        entry.as_module().is_some_and(|module| {
            module.sources.iter().any(|source| match source {
                SourceEntry::Include(path) => path.ends_with(file),
                SourceEntry::Source(source) => {
                    source.path.as_deref().is_some_and(|path| path.ends_with(file))
                }
            })
        })
    })
}

// -- Rust ---------------------------------------------------------------------

const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const VENDOR_DIR: &str = "cargo/vendor";

/// Points cargo at the vendored directory instead of the network.
const CARGO_CONFIG: &str = "[source.vendored-sources]\n\
                            directory = \"cargo/vendor\"\n\
                            \n\
                            [source.crates-io]\n\
                            replace-with = \"vendored-sources\"\n";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Crate {
    name: String,
    version: String,
    checksum: String,
}

/// Turn `Cargo.lock` into the sources list flatpak-builder wants.
///
/// This is a transcription: every crate's tarball address and its sha256 are
/// already in the lock file. A git dependency is the one thing that isn't — it
/// would mean cloning the repository to find out what it contains — and that is
/// refused rather than guessed at.
pub fn cargo_sources(lock_text: &str) -> Result<String, VendorError> {
    let crates = parse_cargo_lock(lock_text)?;
    if crates.is_empty() {
        return Err(VendorError::Unsupported(
            "This Cargo.lock lists no dependencies to prepare. That's normal for a \
             project with none — there is nothing to do."
                .into(),
        ));
    }

    let mut entries: Vec<String> = Vec::new();
    for entry in &crates {
        let dest = format!("{VENDOR_DIR}/{}-{}", entry.name, entry.version);
        entries.push(format!(
            "    {{\n\
             \x20       \"type\": \"archive\",\n\
             \x20       \"archive-type\": \"tar-gzip\",\n\
             \x20       \"url\": \"https://static.crates.io/crates/{name}/{name}-{version}.crate\",\n\
             \x20       \"sha256\": \"{checksum}\",\n\
             \x20       \"dest\": \"{dest}\"\n\
             \x20   }}",
            name = entry.name,
            version = entry.version,
            checksum = entry.checksum,
        ));

        // cargo refuses to use a vendored crate without this file. An empty
        // "files" map is what upstream's generator writes too: the package hash
        // above is what actually pins the contents.
        let checksum_json = format!(
            "{{\"package\": \"{}\", \"files\": {{}}}}",
            entry.checksum
        );
        entries.push(format!(
            "    {{\n\
             \x20       \"type\": \"inline\",\n\
             \x20       \"contents\": {contents},\n\
             \x20       \"dest\": \"{dest}\",\n\
             \x20       \"dest-filename\": \".cargo-checksum.json\"\n\
             \x20   }}",
            contents = quote(&checksum_json),
        ));
    }

    // `config.toml`, not `config`. Cargo has wanted the extension since 1.39 and
    // warns twice on every build without it — "`/run/build/<app>/cargo/config` is
    // deprecated in favor of `config.toml`", printed in the middle of a build
    // someone is already nervous about. Upstream's generator still writes the old
    // name; there is no reason to inherit the warning.
    entries.push(format!(
        "    {{\n\
         \x20       \"type\": \"inline\",\n\
         \x20       \"contents\": {contents},\n\
         \x20       \"dest\": \"cargo\",\n\
         \x20       \"dest-filename\": \"config.toml\"\n\
         \x20   }}",
        contents = quote(CARGO_CONFIG),
    ));

    Ok(format!("[\n{}\n]\n", entries.join(",\n")))
}

/// How many crates a lock file would produce, for "this will list 85 crates"
/// before anything is written.
pub fn cargo_crate_count(lock_text: &str) -> Result<usize, VendorError> {
    Ok(parse_cargo_lock(lock_text)?.len())
}

/// Read `Cargo.lock` from a project folder and write `cargo-sources.json` beside
/// it. The whole operation, so the button in the UI is one call and this can be
/// tested without one.
pub fn prepare_cargo(folder: &Path) -> Result<(PathBuf, usize), VendorError> {
    let lock_path = folder.join(Ecosystem::Cargo.lock_file());
    let text = std::fs::read_to_string(&lock_path).map_err(|_| {
        VendorError::Unreadable(format!(
            "{} — build the project once outside Flatpak and it will appear",
            lock_path.display()
        ))
    })?;

    let json = cargo_sources(&text)?;
    let count = cargo_crate_count(&text)?;
    let target = folder.join(Ecosystem::Cargo.sources_file());

    std::fs::write(&target, json)
        .map_err(|err| VendorError::Unsupported(format!("{} couldn't be written: {err}", target.display())))?;
    Ok((target, count))
}

/// Cargo.lock is machine-written TOML with one shape, so it is read line by
/// line rather than by pulling in a TOML parser that would itself have to be
/// vendored for the offline build.
fn parse_cargo_lock(text: &str) -> Result<Vec<Crate>, VendorError> {
    let mut crates = Vec::new();
    let mut name = String::new();
    let mut version = String::new();
    let mut source = String::new();
    let mut checksum = String::new();
    let mut in_package = false;

    let mut finish = |name: &mut String,
                      version: &mut String,
                      source: &mut String,
                      checksum: &mut String|
     -> Result<(), VendorError> {
        if source.starts_with("git+") {
            return Err(VendorError::Unsupported(format!(
                "{name} comes from a Git repository, and this app can only prepare \
                 crates published to crates.io. Use flatpak-cargo-generator.py from \
                 flatpak-builder-tools for this project."
            )));
        }
        // A package with no source is one of the project's own crates: it is
        // built from the source tree, not downloaded.
        if source == CRATES_IO && !checksum.is_empty() {
            crates.push(Crate {
                name: name.clone(),
                version: version.clone(),
                checksum: checksum.clone(),
            });
        }
        name.clear();
        version.clear();
        source.clear();
        checksum.clear();
        Ok(())
    };

    for line in text.lines() {
        let line = line.trim();

        if line == "[[package]]" {
            if in_package {
                finish(&mut name, &mut version, &mut source, &mut checksum)?;
            }
            in_package = true;
            continue;
        }
        if line.starts_with('[') && line != "[[package]]" {
            if in_package {
                finish(&mut name, &mut version, &mut source, &mut checksum)?;
            }
            in_package = false;
            continue;
        }
        if !in_package {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "name" => name = value,
            "version" => version = value,
            "source" => source = value,
            "checksum" => checksum = value,
            _ => {}
        }
    }

    if in_package {
        finish(&mut name, &mut version, &mut source, &mut checksum)?;
    }

    Ok(crates)
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// -- CMake projects that fetch their own dependencies -------------------------

/// A dependency a CMakeLists.txt downloads for itself, which is the C and C++
/// version of the same problem: it works in a terminal and cannot work inside
/// the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CMakeDownload {
    /// The name as CMake knows it, which is also what the option is named after.
    pub name: String,
    /// Which call it came from, for the explanation.
    pub fetch_content: bool,
}

impl CMakeDownload {
    /// The option that tells CMake to use a copy already on disk instead of
    /// fetching it.
    pub fn source_dir_option(&self, path: &str) -> String {
        format!(
            "-DFETCHCONTENT_SOURCE_DIR_{}={path}",
            self.name.to_uppercase()
        )
    }
}

/// Find the dependencies a CMake project downloads while configuring.
///
/// Only the names are wanted, and CMake's own syntax is regular enough to read
/// them out: `FetchContent_Declare(fmt ...)` and `ExternalProject_Add(fmt ...)`
/// both name the dependency first.
pub fn cmake_downloads(text: &str) -> Vec<CMakeDownload> {
    let mut found: Vec<CMakeDownload> = Vec::new();

    for (call, fetch_content) in [
        ("fetchcontent_declare", true),
        ("externalproject_add", false),
    ] {
        let lower = text.to_lowercase();
        let mut from = 0;
        while let Some(at) = lower[from..].find(call) {
            let start = from + at + call.len();
            from = start;

            let rest = text[start..].trim_start();
            if !rest.starts_with('(') {
                continue;
            }
            let name: String = rest[1..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
                .collect();
            if name.is_empty() || found.iter().any(|entry| entry.name == name) {
                continue;
            }
            found.push(CMakeDownload {
                name,
                fetch_content,
            });
        }
    }

    found
}

/// What a CMake project downloading its own dependencies has to do instead.
/// There is no tool for this one — the answer is to add each dependency to the
/// manifest as a source and point CMake at it.
pub fn cmake_advice(downloads: &[CMakeDownload]) -> String {
    let names = downloads
        .iter()
        .map(|download| download.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "This project's CMakeLists.txt downloads {names} while it configures, and a \
         Flatpak build has no internet. Add each one to “Where the code comes from” as an \
         archive with its address and checksum, then tell CMake to use the copy instead of \
         fetching it: add {} to the build options, and one line per dependency naming where \
         it went, such as {}.",
        FETCHCONTENT_OFFLINE,
        downloads
            .first()
            .map(|download| download.source_dir_option("/run/build/app/deps/name"))
            .unwrap_or_default()
    )
}

/// The option that stops CMake reaching for the network at all. Without it a
/// missing copy is a download attempt rather than an error you can read.
pub const FETCHCONTENT_OFFLINE: &str = "-DFETCHCONTENT_FULLY_DISCONNECTED=ON";

// -- wiring it into the manifest ---------------------------------------------

/// Point the manifest at a prepared list, and set up whatever else that
/// ecosystem needs to build offline. Doing nothing when it is already wired in,
/// so pressing the button twice is harmless.
pub fn wire_in(manifest: &mut Manifest, ecosystem: Ecosystem) {
    let file = ecosystem.sources_file();
    let module_name = manifest
        .main_module()
        .map(|module| module.name.clone())
        .unwrap_or_else(|| "app".to_string());

    let Some(module) = manifest.main_module_mut() else {
        return;
    };

    if !module.sources.iter().any(|source| match source {
        SourceEntry::Include(path) => path.ends_with(file),
        SourceEntry::Source(source) => source.path.as_deref().is_some_and(|p| p.ends_with(file)),
    }) {
        module.sources.push(SourceEntry::Include(file.to_string()));
    }

    match ecosystem {
        Ecosystem::Cargo => wire_cargo(module, &module_name),
        Ecosystem::Node | Ecosystem::Python => {}
    }

    // Setting build-options on the module must not cost it the path to the
    // compiler; syncing again puts the paths in both blocks.
    crate::runtimes::sync_build_paths(manifest);
}

/// Rust needs two more things: somewhere to put the vendored crates, and build
/// commands that don't try to reach the network.
fn wire_cargo(module: &mut Module, module_name: &str) {
    let cargo_home = format!("/run/build/{module_name}/cargo");
    let options = module.build_options.get_or_insert_with(BuildOptions::default);
    options
        .env
        .insert("CARGO_HOME".into(), cargo_home.as_str().into());

    for command in module.build_commands.iter_mut() {
        if command.starts_with("cargo ") && !command.contains("--offline") {
            *command = command.replacen("cargo ", "cargo --offline ", 1);
        }
    }

    // A project whose commands were written by hand may have none that fetch;
    // adding the fetch step makes the failure mode obvious rather than subtle.
    let fetches = module
        .build_commands
        .iter()
        .any(|command| command.contains("cargo --offline fetch"));
    if !fetches
        && module
            .build_commands
            .iter()
            .any(|command| command.starts_with("cargo "))
    {
        module.build_commands.insert(
            0,
            "cargo --offline fetch --manifest-path Cargo.toml --verbose".to_string(),
        );
    }
}

// -- a small library of ready-made modules ------------------------------------

/// Pieces people commonly need to add by hand, written out so nobody has to
/// remember the shape of them.
pub struct Snippet {
    pub name: &'static str,
    pub description: &'static str,
    pub yaml: &'static str,
}

pub const SNIPPETS: &[Snippet] = &[
    Snippet {
        name: "A library built with Meson",
        description: "A dependency your app needs that isn't in the runtime, downloaded \
                      as an archive and built before your app.",
        yaml: "name: mylib\n\
               buildsystem: meson\n\
               config-opts:\n\
               \x20 - -Dtests=false\n\
               sources:\n\
               \x20 - type: archive\n\
               \x20   url: https://example.org/mylib-1.0.tar.xz\n\
               \x20   sha256: 0000000000000000000000000000000000000000000000000000000000000000\n",
    },
    Snippet {
        name: "A library built with configure and make",
        description: "The classic setup, for older dependencies.",
        yaml: "name: mylib\n\
               buildsystem: autotools\n\
               sources:\n\
               \x20 - type: archive\n\
               \x20   url: https://example.org/mylib-1.0.tar.gz\n\
               \x20   sha256: 0000000000000000000000000000000000000000000000000000000000000000\n",
    },
    Snippet {
        name: "A Python package from PyPI",
        description: "One package, installed into the app. For a list of them, prepare \
                      the dependencies instead.",
        yaml: "name: python-requests\n\
               buildsystem: simple\n\
               build-commands:\n\
               \x20 - pip3 install --no-index --find-links=\"file://${PWD}\" \
               --prefix=${FLATPAK_DEST} requests\n\
               sources:\n\
               \x20 - type: file\n\
               \x20   url: https://files.pythonhosted.org/packages/.../requests-2.32.3.tar.gz\n\
               \x20   sha256: 0000000000000000000000000000000000000000000000000000000000000000\n",
    },
    Snippet {
        name: "Someone else's ready-made module",
        description: "The shared-modules collection has tested definitions for common \
                      libraries. This includes one of them.",
        yaml: "shared-modules/glew/glew.json\n",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    const LOCK: &str = r#"
version = 4

[[package]]
name = "myapp"
version = "0.1.0"
dependencies = [
 "serde",
]

[[package]]
name = "serde"
version = "1.0.210"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "c8e3592472072e6e22e0a54d5904d9febf8508f65fb8552499a1abc7d1078c3a"

[[package]]
name = "itoa"
version = "1.0.11"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "49f1f14873335454500d59611f1cf4a4b0f786f9ac11f4312a78e4cf2566695b"
"#;

    #[test]
    fn only_downloaded_crates_end_up_in_the_list() {
        let crates = parse_cargo_lock(LOCK).unwrap();
        // The project's own crate has no source, so it isn't downloaded.
        assert_eq!(crates.len(), 2);
        assert_eq!(crates[0].name, "serde");
        assert_eq!(crates[0].version, "1.0.210");
        assert_eq!(cargo_crate_count(LOCK).unwrap(), 2);
    }

    #[test]
    fn the_list_has_an_address_and_a_checksum_for_every_crate() {
        let json = cargo_sources(LOCK).unwrap();
        assert!(json.starts_with("[\n"));
        assert!(json.trim_end().ends_with(']'));

        assert!(json.contains(
            "\"url\": \"https://static.crates.io/crates/serde/serde-1.0.210.crate\""
        ));
        assert!(json.contains(
            "\"sha256\": \"c8e3592472072e6e22e0a54d5904d9febf8508f65fb8552499a1abc7d1078c3a\""
        ));
        assert!(json.contains("\"dest\": \"cargo/vendor/serde-1.0.210\""));
        // Two entries per crate, plus the cargo config.
        assert_eq!(json.matches("\"type\":").count(), 2 * 2 + 1);
        assert!(json.contains("replace-with"));
    }

    #[test]
    fn what_it_writes_is_valid_json() {
        let json = cargo_sources(LOCK).unwrap();
        // JSON is YAML, so the parser the app already has can check it.
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&json).unwrap();
        let entries = value.as_sequence().unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[0].get("archive-type").and_then(|v| v.as_str()),
            Some("tar-gzip")
        );
        // The inline checksum file is itself readable JSON.
        let contents = entries[1].get("contents").unwrap().as_str().unwrap();
        let inner: serde_yaml_ng::Value = serde_yaml_ng::from_str(contents).unwrap();
        assert!(inner.get("package").is_some());
    }

    #[test]
    fn a_git_dependency_is_refused_rather_than_guessed_at() {
        let lock = "[[package]]\nname = \"thing\"\nversion = \"0.1.0\"\n\
                    source = \"git+https://example.org/thing.git#abc\"\n";
        let err = cargo_sources(lock).unwrap_err();
        assert!(err.friendly().contains("Git repository"));
        assert!(err.friendly().contains("flatpak-cargo-generator"));
    }

    #[test]
    fn a_project_with_no_dependencies_says_so_rather_than_writing_an_empty_file() {
        let lock = "[[package]]\nname = \"myapp\"\nversion = \"0.1.0\"\n";
        assert!(cargo_sources(lock).unwrap_err().friendly().contains("nothing to do"));
    }

    fn manifest_with_cargo() -> Manifest {
        manifest::parse_str(
            "app-id: no.oyzmo.Sample\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: sample\nmodules:\n  - name: sample\n\
             \x20   buildsystem: simple\n    build-commands:\n      - cargo build --release\n\
             \x20   sources:\n      - type: dir\n        path: .\n",
        )
        .unwrap()
        .manifest
    }

    #[test]
    fn wiring_rust_in_does_all_three_things_at_once() {
        let mut manifest = manifest_with_cargo();
        wire_in(&mut manifest, Ecosystem::Cargo);

        let module = manifest.main_module().unwrap();
        // The list is included…
        assert!(module
            .sources
            .iter()
            .any(|source| matches!(source, SourceEntry::Include(path) if path == "cargo-sources.json")));
        // …cargo is told where to put the crates…
        assert_eq!(
            module.build_options.as_ref().unwrap().env["CARGO_HOME"]
                .as_str()
                .unwrap(),
            "/run/build/sample/cargo"
        );
        // …and it is told not to reach for the network.
        assert!(module.build_commands[0].contains("cargo --offline fetch"));
        assert!(module.build_commands[1].starts_with("cargo --offline build"));
        assert!(is_wired(&manifest, "cargo-sources.json"));
    }

    #[test]
    fn wiring_twice_changes_nothing_the_second_time() {
        let mut manifest = manifest_with_cargo();
        wire_in(&mut manifest, Ecosystem::Cargo);
        let once = manifest.clone();
        wire_in(&mut manifest, Ecosystem::Cargo);
        assert_eq!(manifest, once);
    }

    #[test]
    fn what_the_project_needs_comes_from_the_folder_and_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.lock"), LOCK).unwrap();

        let manifest = manifest_with_cargo();
        let found = needs(ProjectKind::Rust, Some(dir.path()), &manifest);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].ecosystem, Ecosystem::Cargo);
        assert!(found[0].lock_path.is_some());
        assert!(found[0].sources_path.is_none());
        assert!(!found[0].is_ready());
        assert!(found[0].state().contains("Not prepared yet"));

        std::fs::write(dir.path().join("cargo-sources.json"), "[]").unwrap();
        let mut manifest = manifest;
        wire_in(&mut manifest, Ecosystem::Cargo);
        let prepared = needs(ProjectKind::Rust, Some(dir.path()), &manifest);
        assert!(prepared[0].is_ready());
        assert!(prepared[0].state().starts_with("Ready"));
    }

    /// A missing lock file names itself and says what produces it — and what
    /// produces it is different for each of the three. The pane shows this row
    /// and nothing else, so anything left out here is left out of the app.
    #[test]
    fn a_missing_lock_file_says_which_one_and_where_it_comes_from() {
        let dir = tempfile::tempdir().unwrap();
        let empty = |kind| needs(kind, Some(dir.path()), &Manifest::default());

        for (kind, file, produces) in [
            (ProjectKind::Rust, "Cargo.lock", "cargo writes it"),
            (ProjectKind::Node, "package-lock.json", "npm install"),
            (ProjectKind::Python, "requirements.txt", "by hand"),
        ] {
            let found = empty(kind);
            assert_eq!(found.len(), 1, "{file}");
            let state = found[0].state();
            assert!(state.contains(file), "{state}");
            assert!(state.contains(produces), "{state}");
        }

        // The old wording sent everyone off to build the project, which produces
        // nothing at all for Python.
        let python = empty(ProjectKind::Python);
        assert!(!python[0].state().contains("Build the project"));
    }

    #[test]
    fn a_cmake_project_that_fetches_its_own_dependencies_is_recognised() {
        let cmake = "cmake_minimum_required(VERSION 3.20)\n\
                     project(thing)\n\
                     include(FetchContent)\n\
                     FetchContent_Declare(fmt\n\
                     \x20 URL https://github.com/fmtlib/fmt/archive/10.2.1.tar.gz)\n\
                     FetchContent_Declare( json GIT_REPOSITORY https://github.com/x/json )\n\
                     ExternalProject_Add(zlib URL https://zlib.net/zlib.tar.gz)\n\
                     find_package(CURL REQUIRED)\n";

        let downloads = cmake_downloads(cmake);
        let names: Vec<&str> = downloads.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["fmt", "json", "zlib"]);
        assert!(downloads[0].fetch_content);
        assert!(!downloads[2].fetch_content);

        // find_package looks at what the SDK already has, and is not a download.
        assert!(cmake_downloads("find_package(CURL REQUIRED)\n").is_empty());
        assert!(cmake_downloads("").is_empty());
    }

    #[test]
    fn the_advice_names_the_dependencies_and_the_options() {
        let downloads = cmake_downloads("FetchContent_Declare(fmt URL https://x/y.tar.gz)\n");
        let advice = cmake_advice(&downloads);

        assert!(advice.contains("fmt"));
        assert!(advice.contains("no internet"));
        assert!(advice.contains(FETCHCONTENT_OFFLINE));
        assert_eq!(
            downloads[0].source_dir_option("/run/build/app/deps/fmt"),
            "-DFETCHCONTENT_SOURCE_DIR_FMT=/run/build/app/deps/fmt"
        );
    }

    #[test]
    fn projects_that_download_nothing_need_nothing() {
        let manifest = manifest_with_cargo();
        assert!(needs(ProjectKind::Meson, None, &manifest).is_empty());
        assert!(needs(ProjectKind::Unknown, None, &manifest).is_empty());
    }

    #[test]
    fn the_ecosystems_this_app_cannot_do_itself_name_the_tool_that_can() {
        assert!(Ecosystem::Cargo.can_generate_here());
        assert!(Ecosystem::Cargo.external_command().is_none());

        for ecosystem in [Ecosystem::Node, Ecosystem::Python] {
            assert!(!ecosystem.can_generate_here());
            let command = ecosystem.external_command().unwrap();
            assert!(command.contains(ecosystem.sources_file()));
            assert!(ecosystem.explanation().contains("no internet"));
        }
    }

    #[test]
    fn every_ready_made_module_is_valid_yaml() {
        for snippet in SNIPPETS {
            assert!(!snippet.description.is_empty());
            let text = format!("app-id: a.b.C\nmodules:\n  - {}", snippet.yaml.replace('\n', "\n    "));
            let import = manifest::parse_str(&text)
                .unwrap_or_else(|err| panic!("{}: {}", snippet.name, err.friendly()));
            assert_eq!(import.manifest.modules.len(), 1, "{}", snippet.name);
        }
    }
}
