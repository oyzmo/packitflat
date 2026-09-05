//! What's wrong with the project, said in plain English.
//!
//! Every check answers three questions: what is wrong, why the rule exists, and
//! what to type instead. A validator that only says "invalid app ID" is exactly
//! the kind of thing this app exists to replace.
//!
//! Errors block generating a manifest; warnings never do. Nothing here refuses
//! to explain itself.

use crate::manifest::Manifest;
use crate::project::Project;
use crate::spdx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The manifest would not build, or would be rejected. Blocks generation.
    Error,
    /// Worth fixing, but the build works. Never blocks.
    Warning,
}

/// Which field to jump to. The wizard maps these to steps, so a problem found
/// on step 4 can send the user back to step 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    AppId,
    Summary,
    Description,
    License,
    Homepage,
    Developer,
    Runtime,
    Sources,
    BuildSystem,
    Command,
    Dependencies,
    Permissions,
    Categories,
    Icon,
    Release,
}

impl Field {
    /// The wizard step this field lives on, 0-based.
    pub fn step(&self) -> u32 {
        match self {
            Field::Name
            | Field::AppId
            | Field::Summary
            | Field::Description
            | Field::License
            | Field::Homepage
            | Field::Developer => 0,
            Field::Runtime => 1,
            Field::Sources => 2,
            Field::BuildSystem | Field::Command => 3,
            Field::Dependencies => 4,
            Field::Permissions => 5,
            Field::Categories | Field::Icon | Field::Release => 6,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Field::Name => "App name",
            Field::AppId => "App ID",
            Field::Summary => "Short description",
            Field::Description => "Longer description",
            Field::License => "Licence",
            Field::Homepage => "Website",
            Field::Developer => "Your name",
            Field::Runtime => "Runtime",
            Field::Sources => "Where the code comes from",
            Field::BuildSystem => "How it's built",
            Field::Command => "Program to start",
            Field::Dependencies => "What it downloads while building",
            Field::Permissions => "Permissions",
            Field::Categories => "Where it appears in the menu",
            Field::Icon => "The app's picture",
            Field::Release => "Version",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub field: Field,
    pub severity: Severity,
    /// What is wrong, in one sentence.
    pub message: String,
    /// What to do about it.
    pub fix: String,
}

impl Issue {
    fn error(field: Field, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Issue {
            field,
            severity: Severity::Error,
            message: message.into(),
            fix: fix.into(),
        }
    }

    fn warning(field: Field, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Issue {
            field,
            severity: Severity::Warning,
            message: message.into(),
            fix: fix.into(),
        }
    }
}

/// Convenience over a list of issues.
pub trait Issues {
    fn errors(&self) -> usize;
    fn warnings(&self) -> usize;
    fn for_step(&self, step: u32) -> Vec<&Issue>;
    fn worst_for_step(&self, step: u32) -> Option<Severity>;
}

impl Issues for [Issue] {
    fn errors(&self) -> usize {
        self.iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    fn warnings(&self) -> usize {
        self.iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }

    fn for_step(&self, step: u32) -> Vec<&Issue> {
        self.iter().filter(|i| i.field.step() == step).collect()
    }

    fn worst_for_step(&self, step: u32) -> Option<Severity> {
        self.for_step(step)
            .iter()
            .map(|i| i.severity)
            .min_by_key(|s| match s {
                Severity::Error => 0,
                Severity::Warning => 1,
            })
    }
}

/// The whole project, front to back.
pub fn project(project: &Project) -> Vec<Issue> {
    let mut issues = Vec::new();

    if project.name.trim().is_empty() {
        issues.push(Issue::error(
            Field::Name,
            "The app doesn't have a name yet.",
            "Type the name people will see under the icon, such as “Pack It Flat”.",
        ));
    }

    issues.extend(app_id(&project.manifest.app_id));
    issues.extend(metadata(project));
    issues.extend(manifest(&project.manifest));
    issues.extend(permissions(&project.manifest.finish_args));
    issues.extend(appearance(project));
    issues.extend(dependencies(project));
    issues.extend(sources_reach_the_build_files(project));
    issues.extend(installs_what_it_builds(project));
    issues
}

/// A build that never installs anything compiles perfectly and then fails on its
/// very last line — `ninja: error: unknown target 'install'`. flatpak-builder
/// always runs that step, because a program left in the build directory is a
/// program that isn't in the finished app.
///
/// It reads like a Flatpak problem and is nothing of the kind: the manifest is
/// right, and the missing line is in the project's own build files. That is
/// exactly the failure worth catching before the build rather than after it.
fn installs_what_it_builds(project: &Project) -> Vec<Issue> {
    let Some(folder) = project.source_dir.as_deref() else {
        return Vec::new();
    };
    let Some(module) = project.manifest.main_module() else {
        return Vec::new();
    };

    // Only when flatpak-builder is the one running the install step. A "simple"
    // module's build-commands do their own installing, and `no-make-install`
    // says the manifest has taken the job over deliberately.
    if !matches!(
        module.buildsystem,
        Some(crate::manifest::BuildSystem::CMake)
            | Some(crate::manifest::BuildSystem::CMakeNinja)
            | Some(crate::manifest::BuildSystem::Meson)
    ) {
        return Vec::new();
    }
    if module
        .extra
        .get("no-make-install")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Vec::new();
    }

    let kind = crate::detect::detect(folder).kind;
    if crate::detect::declares_install(folder, kind) != Some(false) {
        return Vec::new();
    }

    let command = if project.manifest.command.trim().is_empty() {
        "your-program"
    } else {
        project.manifest.command.trim()
    };
    let id = if project.manifest.app_id.trim().is_empty() {
        "your.app.Id"
    } else {
        project.manifest.app_id.trim()
    };

    let (file, lines) = match kind {
        crate::detect::ProjectKind::Meson => (
            "meson.build",
            format!(
                "Add `install : true` to the `executable('{command}', …)` line, then \
                 `install_data('{id}.desktop', install_dir : get_option('datadir') / \
                 'applications')`, the same for {id}.metainfo.xml into `metainfo`, and \
                 `install_subdir('icons', install_dir : get_option('datadir'))`."
            ),
        ),
        _ => (
            "CMakeLists.txt",
            format!(
                "Add these lines to the end of CMakeLists.txt:\n\n\
                 include(GNUInstallDirs)\n\
                 install(TARGETS {command} RUNTIME DESTINATION ${{CMAKE_INSTALL_BINDIR}})\n\
                 install(FILES {id}.desktop\n        \
                 DESTINATION ${{CMAKE_INSTALL_DATADIR}}/applications)\n\
                 install(FILES {id}.metainfo.xml\n        \
                 DESTINATION ${{CMAKE_INSTALL_DATADIR}}/metainfo)\n\
                 install(DIRECTORY icons DESTINATION ${{CMAKE_INSTALL_DATADIR}})"
            ),
        ),
    };

    vec![Issue::warning(
        Field::BuildSystem,
        format!("{file} never says where the finished program goes."),
        format!(
            "The build will compile {command} and then stop with “unknown target \
             'install'”, because nothing tells it to put the program anywhere. \
             The Flatpak build installs into /app, so the program has to land in \
             /app/bin/{command} and the desktop entry and metainfo beside it — \
             otherwise the app builds but never appears in the menu. {lines}"
        ),
    )]
}

/// The build copies in exactly what the sources say and nothing else. Pointing a
/// folder source at a subdirectory — `src` rather than `.` — leaves Cargo.toml,
/// meson.build or the makefile outside the build, and the failure that follows
/// blames the build system rather than the source.
fn sources_reach_the_build_files(project: &Project) -> Vec<Issue> {
    let Some(folder) = project.source_dir.as_deref() else {
        return Vec::new();
    };
    let Some(marker) = crate::detect::detect(folder).evidence else {
        return Vec::new();
    };
    // Only worth saying when the file really is where the sources aren't.
    if !folder.join(&marker).is_file() {
        return Vec::new();
    }
    let Some(module) = project.manifest.main_module() else {
        return Vec::new();
    };

    module
        .sources
        .iter()
        .filter_map(|entry| entry.as_source())
        .filter(|source| source.kind == crate::manifest::SourceKind::Dir)
        .filter_map(|source| {
            let path = source.path.as_deref()?.trim();
            if path.is_empty() || path == "." {
                return None;
            }
            let copied = folder.join(path);
            (!copied.join(&marker).is_file()).then(|| {
                Issue::warning(
                    Field::Sources,
                    format!("The build only copies “{path}”, and {marker} isn't in there."),
                    format!(
                        "A build starts with an empty folder and copies in exactly what \
                         the sources say. {marker} sits beside “{path}”, not inside it, \
                         so the build won't find it. Use “.” — the whole project folder — \
                         unless you have a reason not to."
                    ),
                )
            })
        })
        .collect()
}

/// A project that downloads its dependencies while building will fail inside the
/// sandbox, for a reason nobody guesses on their own. Saying so here is the
/// whole point — the failure otherwise arrives twenty minutes into a build.
fn dependencies(project: &Project) -> Vec<Issue> {
    let Some(folder) = project.source_dir.as_deref() else {
        return Vec::new();
    };
    let kind = crate::detect::detect(folder).kind;

    // C and C++ have no lock file, but a CMakeLists.txt that fetches its own
    // dependencies hits the same wall — and the build gets a long way in before
    // it does.
    let mut issues = Vec::new();
    if let Ok(text) = std::fs::read_to_string(folder.join("CMakeLists.txt")) {
        let downloads = crate::vendor::cmake_downloads(&text);
        if !downloads.is_empty()
            && !project
                .manifest
                .main_module()
                .is_some_and(|module| {
                    module.config_opts.iter().any(|opt| {
                        opt.contains("FETCHCONTENT_FULLY_DISCONNECTED")
                            || opt.contains("FETCHCONTENT_SOURCE_DIR")
                    })
                })
        {
            issues.push(Issue::warning(
                Field::Dependencies,
                format!(
                    "This project downloads {} for itself while building.",
                    downloads
                        .iter()
                        .map(|download| download.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                crate::vendor::cmake_advice(&downloads),
            ));
        }
    }

    issues.extend(
        crate::vendor::needs(kind, Some(folder), &project.manifest)
        .into_iter()
        .filter(|need| need.lock_path.is_some() && !need.is_ready())
        .map(|need| {
            Issue::warning(
                Field::Dependencies,
                format!(
                    "The {} this project uses haven't been written down yet.",
                    need.ecosystem.label().to_lowercase()
                ),
                format!(
                    "{} {}",
                    need.ecosystem.explanation(),
                    "Until that is done the build stops as soon as it tries to download \
                     anything."
                ),
            )
        }),
    );

    issues
}

/// What the app would be allowed to do. None of this stops a build — an app that
/// asks for everything builds perfectly well — so these are warnings written to
/// be read rather than dismissed.
pub fn permissions(finish_args: &[String]) -> Vec<Issue> {
    use crate::permissions::{Level, Permissions};

    Permissions::parse(finish_args)
        .risks()
        .into_iter()
        .filter(|risk| risk.level >= Level::Notable)
        .map(|risk| Issue {
            field: Field::Permissions,
            severity: Severity::Warning,
            message: risk.headline,
            fix: match risk.instead {
                Some(instead) => format!("{} {instead}", risk.detail),
                None => risk.detail,
            },
        })
        .collect()
}

/// The things a store shows. Missing ones don't stop a build — but three of them
/// decide whether the app *has* a store listing at all.
///
/// **A summary, a description and a category are what AppStream insists on.**
/// The app information file is installed into the finished app, and
/// flatpak-builder hands it to `appstreamcli compose`, which refuses a listing
/// missing any of the three and takes the whole build down with it. So
/// `generate` leaves an incomplete one out of the build rather than failing it,
/// and these say what that costs. All three measured against flatpak-builder
/// 1.4.10 and the GNOME 50 SDK, one field at a time.
fn appearance(project: &Project) -> Vec<Issue> {
    let mut issues = Vec::new();

    if project.description.trim().is_empty() {
        issues.push(Issue::warning(
            Field::Description,
            "There's no longer description.",
            "A few sentences saying what the app does. It's the main thing someone \
             reads before installing, and without it the app information is left \
             out of the build, so app stores show nothing about the app.",
        ));
    }

    if project.categories.is_empty() {
        issues.push(Issue::warning(
            Field::Categories,
            "The app doesn't say where it belongs in the menu.",
            "Pick at least one category, or it turns up under “Other” — and the app \
             information is left out of the build until there is one.",
        ));
    } else if let Some(unknown) = project
        .categories
        .iter()
        .find(|category| !crate::appdata::CATEGORIES.iter().any(|(id, _)| id == *category))
    {
        issues.push(Issue::warning(
            Field::Categories,
            format!("“{unknown}” isn't a category desktops recognise."),
            "Pick from the list. Anything else is ignored, and the app lands under \
             “Other”.",
        ));
    }

    match &project.icon_source {
        None => issues.push(Issue::warning(
            Field::Icon,
            "The app has no picture.",
            "Without one it shows as a blank square in the menu and in stores. An SVG \
             is best; a square PNG of 128 pixels or more also works.",
        )),
        Some(path) => match crate::icons::inspect(path) {
            Err(err) => issues.push(Issue::warning(
                Field::Icon,
                err.friendly(),
                "Choose an SVG or a PNG. If the file has moved, pick it again.",
            )),
            Ok(kind) => {
                for note in crate::icons::advice(&kind) {
                    issues.push(Issue::warning(
                        Field::Icon,
                        note,
                        "An SVG avoids all of this: it is drawn at whatever size is needed.",
                    ));
                }
            }
        },
    }

    if project.release_version.trim().is_empty() {
        issues.push(Issue::warning(
            Field::Release,
            "No version has been given.",
            "Stores show a version and a date, and refuse apps without one. Something \
             like 1.0.0 is fine to start with.",
        ));
    }

    issues
}

/// Flatpak's rules for an app ID, each with the reason behind it. The ID also
/// becomes a D-Bus name, which is where the stricter rules come from.
pub fn app_id(id: &str) -> Vec<Issue> {
    let mut issues = Vec::new();

    if id.trim().is_empty() {
        issues.push(Issue::error(
            Field::AppId,
            "Every Flatpak needs an app ID.",
            "Use your website address backwards, then the app's name: a site at \
             oyzmo.no gives “no.oyzmo.PackItFlat”. If you have no website, a code \
             hosting account works: “io.github.yourname.YourApp”.",
        ));
        return issues;
    }

    let segments: Vec<&str> = id.split('.').collect();

    if segments.len() < 3 {
        issues.push(Issue::error(
            Field::AppId,
            format!(
                "“{id}” has {} part{}, and an app ID needs at least three, separated by dots.",
                segments.len(),
                if segments.len() == 1 { "" } else { "s" }
            ),
            "The parts are your website address backwards followed by the app's \
             name: “no.oyzmo.PackItFlat”.",
        ));
    }

    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();

        if segment.is_empty() {
            issues.push(Issue::error(
                Field::AppId,
                "There are two dots next to each other, or the ID starts or ends with a dot.",
                "Every part between the dots has to have something in it.",
            ));
            continue;
        }

        if segment.starts_with(|c: char| c.is_ascii_digit()) {
            issues.push(Issue::error(
                Field::AppId,
                format!("The part “{segment}” starts with a number, which isn't allowed."),
                "Put a letter or an underscore first — “com.2ndlab.App” has to become \
                 “com._2ndlab.App”.",
            ));
        }

        if let Some(bad) = segment
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
        {
            issues.push(Issue::error(
                Field::AppId,
                format!("The part “{segment}” contains “{bad}”, which isn't allowed in an app ID."),
                "Only letters, numbers and underscores are safe. Leave out spaces, \
                 accents and punctuation.",
            ));
        }

        if segment.contains('-') {
            if last {
                issues.push(Issue::error(
                    Field::AppId,
                    format!("The last part, “{segment}”, has a hyphen in it."),
                    "The app ID is also used as the app's name on the system message \
                     bus, which doesn't allow hyphens in that position. Use an \
                     underscore, or run the words together: “PackItFlat”.",
                ));
            } else {
                issues.push(Issue::warning(
                    Field::AppId,
                    format!("The part “{segment}” has a hyphen in it."),
                    "It will work, but hyphens cause trouble elsewhere — an \
                     underscore is safer.",
                ));
            }
        }
    }

    if id.len() > 255 {
        issues.push(Issue::error(
            Field::AppId,
            "The app ID is longer than 255 characters.",
            "Shorten it — the parts before the app's name are usually just a domain.",
        ));
    }

    if id.ends_with(".desktop") {
        issues.push(Issue::warning(
            Field::AppId,
            "The app ID ends with “.desktop”.",
            "That's the old naming style. Drop the “.desktop” — the desktop file is \
             named after the ID, not the other way round.",
        ));
    }

    let placeholder = ["com.example", "org.example", "io.github.username", "com.company"];
    if placeholder.iter().any(|p| id.starts_with(p)) {
        issues.push(Issue::warning(
            Field::AppId,
            "This app ID uses an example domain.",
            "Change it to a domain or code-hosting account you actually control, \
             otherwise the ID belongs to someone else.",
        ));
    }

    issues
}

/// The things that describe the app to people rather than to the builder. They
/// end up in the app's listing, so they are warnings, never errors.
fn metadata(project: &Project) -> Vec<Issue> {
    let mut issues = Vec::new();

    let summary = project.summary.trim();
    if summary.is_empty() {
        issues.push(Issue::warning(
            Field::Summary,
            "There's no short description.",
            "One line saying what the app does, shown under its name in app stores: \
             “Make a Flatpak without knowing what a manifest is”. Until it's there, \
             the app information is written but left out of the build — see the \
             review step.",
        ));
    } else {
        if summary.len() > 35 {
            issues.push(Issue::warning(
                Field::Summary,
                format!("The short description is {} characters long.", summary.len()),
                "App stores cut it off around 35. Keep the important half first.",
            ));
        }
        if summary.ends_with('.') {
            issues.push(Issue::warning(
                Field::Summary,
                "The short description ends with a full stop.",
                "It's a label rather than a sentence — leave the full stop off.",
            ));
        }
        if summary
            .to_lowercase()
            .starts_with(&project.name.to_lowercase())
            && !project.name.trim().is_empty()
        {
            issues.push(Issue::warning(
                Field::Summary,
                "The short description starts with the app's own name.",
                "The name is already shown right above it. Say what the app does instead.",
            ));
        }
    }

    let license = project.license.trim();
    if license.is_empty() {
        issues.push(Issue::warning(
            Field::License,
            "No licence has been chosen.",
            "The licence says what other people may do with your code. If you don't \
             know, “GPL-3.0-or-later” keeps the app and its changes open.",
        ));
    } else if !spdx::is_known(license) {
        issues.push(Issue::warning(
            Field::License,
            format!("“{license}” isn't a licence name app stores recognise."),
            "Pick one from the list. The names come from a standard catalogue, so \
             “GPLv3” has to be written “GPL-3.0-or-later”.",
        ));
    }

    let homepage = project.homepage.trim();
    if !homepage.is_empty() && !homepage.starts_with("http://") && !homepage.starts_with("https://")
    {
        issues.push(Issue::warning(
            Field::Homepage,
            "The website address doesn't start with http:// or https://.",
            "Write the whole address, the way it looks in a browser's address bar.",
        ));
    }

    if project.developer.trim().is_empty() {
        issues.push(Issue::warning(
            Field::Developer,
            "Nobody is named as the app's developer.",
            "App stores show this next to the app. A name or a project name is fine.",
        ));
    }

    issues
}

/// The parts flatpak-builder itself needs.
fn manifest(manifest: &Manifest) -> Vec<Issue> {
    let mut issues = Vec::new();

    if manifest.runtime.trim().is_empty() || manifest.runtime_version.trim().is_empty() {
        issues.push(Issue::error(
            Field::Runtime,
            "No runtime has been chosen.",
            "The runtime is the set of libraries your app runs on. GNOME is the \
             usual choice for a desktop app.",
        ));
    }
    if manifest.sdk.trim().is_empty() {
        issues.push(Issue::error(
            Field::Runtime,
            "No SDK has been chosen.",
            "The SDK is the runtime plus the tools needed to compile against it. \
             It's picked for you when you choose a runtime.",
        ));
    }

    // The failure this catches: a compiler switched on, downloaded and installed,
    // and then not found by the build because nothing put it on the path.
    let append_path = manifest
        .build_options
        .as_ref()
        .and_then(|options| options.append_path.as_deref());
    for extension in manifest
        .sdk_extensions
        .iter()
        .filter_map(|id| crate::runtimes::extension(id))
    {
        let on_module_path = manifest.main_module().is_some_and(|module| {
            module
                .build_options
                .as_ref()
                .and_then(|options| options.append_path.as_deref())
                .is_some_and(|path| crate::runtimes::path_is_set(Some(path), extension))
        });
        if !crate::runtimes::path_is_set(append_path, extension) && !on_module_path {
            issues.push(Issue::warning(
                Field::Runtime,
                format!(
                    "The {} tools are switched on, but nothing tells the build where they are.",
                    extension.label
                ),
                format!(
                    "A build like this downloads everything and then stops with \
                     “command not found”. Switching {} off and on again adds the missing \
                     line: append-path {}.",
                    extension.label, extension.bin_path
                ),
            ));
        }
    }

    if manifest.command.trim().is_empty() {
        issues.push(Issue::error(
            Field::Command,
            "Nothing is set to run when the app is opened.",
            "This is the name of the program that gets installed — usually the same \
             as the module's name.",
        ));
    } else if manifest.command.contains('/') {
        issues.push(Issue::warning(
            Field::Command,
            "The program to start contains a “/”.",
            "Use just the name. Anything installed into /app/bin is found by name.",
        ));
    }

    let Some(module) = manifest.main_module() else {
        issues.push(Issue::error(
            Field::Sources,
            "There's nothing to build.",
            "Add at least one place for the code to come from — usually the folder \
             your project is in.",
        ));
        return issues;
    };

    if module.name.trim().is_empty() {
        issues.push(Issue::error(
            Field::BuildSystem,
            "The part being built doesn't have a name.",
            "A short name with no spaces, such as the program's own name.",
        ));
    } else if module.name.contains(char::is_whitespace) {
        issues.push(Issue::warning(
            Field::BuildSystem,
            "The name of the part being built contains a space.",
            "Use hyphens instead — it becomes a folder name during the build.",
        ));
    }

    if module.sources.is_empty() {
        issues.push(Issue::error(
            Field::Sources,
            "There's nowhere for the code to come from.",
            "Add the folder your project is in, or the address of its Git repository.",
        ));
    }

    for entry in &module.sources {
        let Some(source) = entry.as_source() else {
            continue;
        };
        use crate::manifest::SourceKind;
        match source.kind {
            SourceKind::Git => {
                if source.url.as_deref().unwrap_or("").trim().is_empty() {
                    issues.push(Issue::error(
                        Field::Sources,
                        "A Git source has no address.",
                        "Paste the repository address, the one you would use with \
                         “git clone”.",
                    ));
                }
                if source.tag.is_none() && source.commit.is_none() && source.branch.is_none() {
                    issues.push(Issue::warning(
                        Field::Sources,
                        "A Git source doesn't say which version to build.",
                        "Without a tag or commit the build changes every time the \
                         repository does, and yesterday's build can't be repeated.",
                    ));
                }
            }
            SourceKind::Archive => {
                if source.url.as_deref().unwrap_or("").trim().is_empty() {
                    issues.push(Issue::error(
                        Field::Sources,
                        "A downloaded archive has no address.",
                        "Paste the address of the .tar.gz or .zip file.",
                    ));
                }
                match source.sha256.as_deref() {
                    None | Some("") => issues.push(Issue::error(
                        Field::Sources,
                        "A downloaded archive has no checksum.",
                        "The checksum proves the download is the file you meant. \
                         Compute it from a copy you have, or paste the one the \
                         project publishes.",
                    )),
                    Some(hash) if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) => {
                        issues.push(Issue::error(
                            Field::Sources,
                            "That checksum doesn't look like a SHA-256.",
                            "It should be exactly 64 characters, digits and the \
                             letters a to f.",
                        ))
                    }
                    _ => {}
                }
            }
            SourceKind::Dir if source.path.as_deref().unwrap_or("").trim().is_empty() => {
                issues.push(Issue::error(
                    Field::Sources,
                    "A folder source has no folder.",
                    "Point it at the folder holding the code.",
                ));
            }
            _ => {}
        }
    }

    use crate::manifest::BuildSystem;
    match &module.buildsystem {
        None | Some(BuildSystem::Simple) => {
            if module.build_commands.is_empty() {
                issues.push(Issue::error(
                    Field::BuildSystem,
                    "Nothing says how to build this project.",
                    "Either choose a build system Flatpak knows, or write the \
                     commands that compile and install the program.",
                ));
            }
        }
        Some(_) => {
            if !module.build_commands.is_empty() {
                issues.push(Issue::warning(
                    Field::BuildSystem,
                    "There are build commands as well as a build system.",
                    "The build system runs its own commands; the extra ones are \
                     ignored unless the build system is “simple”.",
                ));
            }
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    fn ids(id: &str) -> Vec<Issue> {
        app_id(id)
    }

    #[test]
    fn a_good_app_id_passes() {
        assert!(ids("no.oyzmo.PackItFlat").is_empty());
        assert!(ids("io.github.someone.Thing").is_empty());
    }

    #[test]
    fn too_few_parts_is_explained() {
        let issues = ids("oyzmo.App");
        assert_eq!(issues.errors(), 1);
        assert!(issues[0].message.contains("at least three"));
        assert!(!issues[0].fix.is_empty());
    }

    #[test]
    fn hyphen_in_the_last_part_is_an_error_elsewhere_a_warning() {
        let last = ids("no.oyzmo.Pack-It-Flat");
        assert_eq!(last.errors(), 1);
        assert!(last[0].fix.contains("message bus"));

        let middle = ids("no.my-domain.App");
        assert_eq!(middle.errors(), 0);
        assert_eq!(middle.warnings(), 1);
    }

    #[test]
    fn digits_punctuation_and_empty_parts_are_caught() {
        assert!(ids("com.2ndlab.App")[0].message.contains("starts with a number"));
        assert!(ids("no.oyzmo.Pack It")[0].message.contains("contains"));
        assert_eq!(ids("no..App").errors(), 1);
        assert_eq!(ids("").errors(), 1);
    }

    #[test]
    fn example_domains_are_flagged_but_not_blocked() {
        let issues = ids("com.example.App");
        assert_eq!(issues.errors(), 0);
        assert_eq!(issues.warnings(), 1);
    }

    /// The sample project as a CMake one, which is the shape that can fail on
    /// the install step: flatpak-builder drives the build itself.
    fn cmake_project(dir: &std::path::Path, cmakelists: &str) -> Project {
        std::fs::write(dir.join("CMakeLists.txt"), cmakelists).unwrap();
        let mut p = sample_project(dir);
        p.manifest.command = "sample".into();
        let module = p.manifest.main_module_mut().unwrap();
        module.buildsystem = Some(manifest::BuildSystem::CMakeNinja);
        module.build_commands.clear();
        p
    }

    #[test]
    fn a_cmake_project_that_installs_nothing_is_warned_about_before_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let p = cmake_project(
            dir.path(),
            "project(sample)\nadd_executable(sample main.cpp)\n",
        );

        let issues = project(&p);
        assert_eq!(issues.errors(), 0, "{issues:#?}");
        assert_eq!(issues.warnings(), 1, "{issues:#?}");
        let issue = &issues.for_step(3)[0];
        assert!(issue.message.contains("CMakeLists.txt"));
        // The lines to type, not just the diagnosis.
        assert!(issue.fix.contains("install(TARGETS sample"));
        assert!(issue.fix.contains("no.oyzmo.Sample.desktop"));
        assert!(issue.fix.contains("no.oyzmo.Sample.metainfo.xml"));
    }

    #[test]
    fn an_install_rule_anywhere_in_the_project_settles_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = cmake_project(
            dir.path(),
            "project(sample)\nadd_subdirectory(src)\n",
        );
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/CMakeLists.txt"),
            "add_executable(sample main.cpp)\ninstall(TARGETS sample)\n",
        )
        .unwrap();

        assert_eq!(project(&p).warnings(), 0, "{:#?}", project(&p));
    }

    /// The manifest taking the job over is not a mistake to report.
    #[test]
    fn no_make_install_in_the_manifest_silences_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = cmake_project(
            dir.path(),
            "project(sample)\nadd_executable(sample main.cpp)\n",
        );
        p.manifest
            .main_module_mut()
            .unwrap()
            .extra
            .insert("no-make-install".into(), true.into());

        assert_eq!(project(&p).warnings(), 0, "{:#?}", project(&p));
    }

    /// A "simple" module does its own installing in build-commands, so the
    /// project's build files having no install rule means nothing.
    #[test]
    fn a_simple_module_is_not_asked_about_install_rules() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CMakeLists.txt"),
            "project(sample)\nadd_executable(sample main.cpp)\n",
        )
        .unwrap();

        assert_eq!(project(&sample_project(dir.path())).warnings(), 0);
    }

    /// A project with every question answered — including the ones about how it
    /// looks in a store, which is what "finished" means here.
    fn sample_project(dir: &std::path::Path) -> Project {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.Sample\n\
             runtime: org.gnome.Platform\n\
             runtime-version: '50'\n\
             sdk: org.gnome.Sdk\n\
             command: sample\n\
             finish-args:\n\
             \x20 - --socket=wayland\n\
             \x20 - --socket=fallback-x11\n\
             \x20 - --share=ipc\n\
             \x20 - --device=dri\n\
             modules:\n\
             \x20 - name: sample\n\
             \x20   buildsystem: simple\n\
             \x20   build-commands:\n\
             \x20     - make install\n\
             \x20   sources:\n\
             \x20     - type: dir\n\
             \x20       path: .\n",
        )
        .unwrap();

        let icon = dir.join("icon.svg");
        std::fs::write(&icon, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();

        let mut p = Project::from_import(&import, None);
        p.name = "Sample".into();
        p.summary = "Does a thing well".into();
        p.description = "It does the thing, and it does it well.".into();
        p.license = "GPL-3.0-or-later".into();
        p.developer = "oyzmo".into();
        p.categories = vec!["Utility".into()];
        p.icon_source = Some(icon);
        p.release_version = "1.0.0".into();
        p.source_dir = Some(dir.to_path_buf());
        p
    }

    #[test]
    fn a_complete_project_has_nothing_to_report() {
        let dir = tempfile::tempdir().unwrap();
        let issues = project(&sample_project(dir.path()));
        assert_eq!(issues.errors(), 0, "{issues:#?}");
        assert_eq!(issues.warnings(), 0, "{issues:#?}");
    }

    #[test]
    fn missing_essentials_are_errors_on_the_right_steps() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project(dir.path());
        p.manifest.command.clear();
        p.manifest.runtime.clear();

        let issues = project(&p);
        assert_eq!(issues.errors(), 2);
        assert_eq!(issues.for_step(1).len(), 1); // runtime, step 2
        assert_eq!(issues.for_step(3).len(), 1); // command, step 4
        assert_eq!(issues.worst_for_step(1), Some(Severity::Error));
        assert_eq!(issues.worst_for_step(0), None);
    }

    #[test]
    fn what_the_app_may_do_is_reported_as_advice_never_as_a_blocker() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project(dir.path());
        p.manifest.finish_args.push("--filesystem=home".into());

        let issues = project(&p);
        assert_eq!(issues.errors(), 0, "permissions never block a build");

        let issue = issues
            .iter()
            .find(|issue| issue.field == Field::Permissions)
            .expect("the home folder is mentioned");
        assert!(issue.message.contains("home folder"));
        assert!(issue.fix.contains("file chooser"));
        assert_eq!(issue.field.step(), 5);
    }

    #[test]
    fn what_a_store_needs_is_asked_for_on_the_appearance_step() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project(dir.path());
        p.description.clear();
        p.categories.clear();
        p.icon_source = None;
        p.release_version.clear();

        let issues = project(&p);
        assert_eq!(issues.errors(), 0);
        assert_eq!(issues.warnings(), 4);
        // The description belongs with the other words, the rest with appearance.
        assert_eq!(issues.for_step(0).len(), 1);
        assert_eq!(issues.for_step(6).len(), 3);
    }

    /// None of the three blocks anything — what they cost is said in the advice,
    /// because `generate` keeps an unfinished listing out of the build rather
    /// than letting the build fail on it.
    #[test]
    fn the_three_fields_a_listing_needs_are_advice_that_says_what_is_lost() {
        let dir = tempfile::tempdir().unwrap();
        for (name, clear) in [
            ("summary", &(|p: &mut Project| p.summary.clear()) as &dyn Fn(&mut Project)),
            ("description", &|p: &mut Project| p.description.clear()),
            ("categories", &|p: &mut Project| p.categories.clear()),
        ] {
            let mut p = sample_project(dir.path());
            clear(&mut p);
            assert!(!crate::appdata::listing_is_complete(&p), "{name}");

            let issues = project(&p);
            assert_eq!(issues.errors(), 0, "{name}: {issues:#?}");
            assert!(
                issues.iter().any(|issue| issue.fix.contains("left out of the build")
                    || issue.fix.contains("left out of the build")),
                "{name}: nothing says what it costs: {issues:#?}"
            );
        }
    }

    /// From a real manifest: the source said `src`, so Cargo.toml would never
    /// have reached the build. The failure it causes names cargo, not the source.
    #[test]
    fn a_source_that_leaves_the_build_file_behind_is_caught() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();

        let mut p = sample_project(dir.path());
        let module = p.manifest.main_module_mut().unwrap();
        module.sources = vec![crate::manifest::SourceEntry::Source(
            crate::manifest::Source::dir("src"),
        )];

        let issues = project(&p);
        let issue = issues
            .iter()
            .find(|issue| issue.field == Field::Sources)
            .expect("the missing Cargo.toml is mentioned");
        assert!(issue.message.contains("Cargo.toml isn't in there"));
        assert!(issue.fix.contains("Use “.”"));

        // Pointing at the whole folder is fine, and says nothing.
        let module = p.manifest.main_module_mut().unwrap();
        module.sources = vec![crate::manifest::SourceEntry::Source(
            crate::manifest::Source::dir("."),
        )];
        assert!(project(&p)
            .iter()
            .all(|issue| issue.field != Field::Sources));
    }

    #[test]
    fn a_picture_that_will_disappoint_is_mentioned_before_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project(dir.path());

        let tiny = dir.path().join("tiny.png");
        let mut png = Vec::from(b"\x89PNG\r\n\x1a\n".as_slice());
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&48u32.to_be_bytes());
        png.extend_from_slice(&48u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        std::fs::write(&tiny, &png).unwrap();
        p.icon_source = Some(tiny);

        let issues = project(&p);
        assert!(issues
            .iter()
            .any(|issue| issue.field == Field::Icon && issue.message.contains("at least 128")));
    }

    #[test]
    fn archive_sources_need_a_real_checksum() {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.A\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: a\nmodules:\n  - name: a\n    buildsystem: meson\n\
             \x20   sources:\n      - type: archive\n        url: https://x/y.tar.xz\n\
             \x20       sha256: nope\n",
        )
        .unwrap();
        let issues = manifest(&import.manifest);
        assert!(issues
            .iter()
            .any(|i| i.message.contains("doesn't look like a SHA-256")));
    }

    #[test]
    fn git_without_a_tag_is_only_a_warning() {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.A\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: a\nmodules:\n  - name: a\n    buildsystem: meson\n\
             \x20   sources:\n      - type: git\n        url: https://example.invalid/a.git\n",
        )
        .unwrap();
        let issues = manifest(&import.manifest);
        assert_eq!(issues.errors(), 0);
        assert_eq!(issues.warnings(), 1);
    }

    #[test]
    fn summary_advice_matches_what_stores_expect() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = sample_project(dir.path());
        p.summary = "Sample is a tool that does a great many useful things.".into();

        let issues = project(&p);
        // too long, ends with a full stop, starts with the app's name
        assert_eq!(issues.for_step(0).len(), 3);
        assert_eq!(issues.errors(), 0);
    }
}
