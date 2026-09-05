//! Running the build, and explaining what happened.
//!
//! Nothing here starts a process: this module decides *what* to run, *whether it
//! can* run, and *what a failure means*. The UI does the running, because that
//! has to be asynchronous; everything that can be got wrong is text in, text
//! out, and tested.
//!
//! The brief's rule holds throughout: no raw error ever reaches the screen. A
//! build that fails produces a sentence, a reason, and — wherever possible — a
//! button that fixes it.

use crate::project::Project;

/// A command to run, already split the way a process wants it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub argv: Vec<String>,
}

impl Command {
    fn new(parts: &[&str]) -> Self {
        Command {
            argv: parts.iter().map(|part| part.to_string()).collect(),
        }
    }

    /// The same command as someone would type it, for the "copy this into a
    /// terminal" fallback that every step here has.
    pub fn as_typed(&self) -> String {
        self.argv
            .iter()
            .map(|part| {
                if part.contains(' ') {
                    format!("\"{part}\"")
                } else {
                    part.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Wrapped so it runs on the host when the app is itself sandboxed. Flatpak
    /// cannot build a Flatpak inside a Flatpak, so this is the only way the
    /// build button can work at all from an installed copy.
    pub fn on_host(&self, sandboxed: bool) -> Command {
        if !sandboxed {
            return self.clone();
        }
        let mut argv = vec![
            "flatpak-spawn".to_string(),
            "--host".to_string(),
            // Without this the build's own output arrives in one lump at the
            // end, which makes a ten-minute build look like a hang.
            "--env=PYTHONUNBUFFERED=1".to_string(),
        ];
        argv.extend(self.argv.iter().cloned());
        Command { argv }
    }
}

/// The switches on the build itself, each with what it means for the person
/// pressing the button rather than for flatpak-builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// `--force-clean`: start from an empty build directory.
    pub clean_first: bool,
    /// `--install-deps-from=flathub`: fetch a missing runtime or SDK rather than
    /// stopping.
    pub fetch_missing: bool,
    /// `--install`: put the finished app on this computer.
    pub install_after: bool,
    /// `--keep-build-dirs`: leave the working files behind for inspection.
    pub keep_build_dirs: bool,
    /// Pack the finished app into `<app-id>.flatpak` beside the manifest. On by
    /// default: a build that leaves nothing but a `build-dir` is a build nobody
    /// asked for — someone who pressed "Build" wants the file.
    pub make_bundle: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            clean_first: true,
            fetch_missing: true,
            install_after: false,
            keep_build_dirs: false,
            make_bundle: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Option_ {
    CleanFirst,
    FetchMissing,
    InstallAfter,
    KeepBuildDirs,
    MakeBundle,
}

impl Option_ {
    pub const ALL: &'static [Option_] = &[
        Option_::CleanFirst,
        Option_::FetchMissing,
        Option_::InstallAfter,
        Option_::KeepBuildDirs,
        Option_::MakeBundle,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Option_::CleanFirst => "Start from scratch",
            Option_::FetchMissing => "Fetch anything that's missing",
            Option_::InstallAfter => "Install it when it's built",
            Option_::KeepBuildDirs => "Keep the working files",
            Option_::MakeBundle => "Make a .flatpak file",
        }
    }

    pub fn explanation(&self) -> &'static str {
        match self {
            Option_::CleanFirst => {
                "Throws away what the last build left behind. Slower, and the only way \
                 to be sure you are building what you think you are."
            }
            Option_::FetchMissing => {
                "Downloads the runtime or SDK from Flathub if this computer hasn't got \
                 them. Needs an internet connection the first time."
            }
            Option_::InstallAfter => {
                "Adds the finished app to this computer, so it appears in the menu. You \
                 can also install it afterwards."
            }
            Option_::KeepBuildDirs => {
                "Leaves the half-built files in place so you can look inside when \
                 something failed. Takes up disk space."
            }
            Option_::MakeBundle => {
                "Packs the finished app into one file beside the manifest, which anyone \
                 can install by double-clicking it. Without this the build leaves only \
                 its working folder behind."
            }
        }
    }

    pub fn get(&self, options: &Options) -> bool {
        match self {
            Option_::CleanFirst => options.clean_first,
            Option_::FetchMissing => options.fetch_missing,
            Option_::InstallAfter => options.install_after,
            Option_::KeepBuildDirs => options.keep_build_dirs,
            Option_::MakeBundle => options.make_bundle,
        }
    }

    pub fn set(&self, options: &mut Options, on: bool) {
        match self {
            Option_::CleanFirst => options.clean_first = on,
            Option_::FetchMissing => options.fetch_missing = on,
            Option_::InstallAfter => options.install_after = on,
            Option_::KeepBuildDirs => options.keep_build_dirs = on,
            Option_::MakeBundle => options.make_bundle = on,
        }
    }
}

/// Where the build's working files go, relative to the project folder.
pub const BUILD_DIR: &str = "build-dir";
pub const REPO_DIR: &str = "build-repo";

pub fn build_command(manifest_file: &str, options: &Options, flathub: Scope) -> Command {
    let mut command = Command::new(&["flatpak-builder"]);
    if options.clean_first {
        command.argv.push("--force-clean".into());
    }
    // The bundle is made from a repository, so the build has to write one as it
    // goes; there is no way to produce the file afterwards from `build-dir`.
    if options.make_bundle {
        command.argv.push(format!("--repo={REPO_DIR}"));
    }
    // Building into the user's installation, which needs no password. Finding
    // the runtime is not affected by this: flatpak-builder looks in both, and a
    // system-wide SDK is used by a `--user` build without complaint. Measured.
    command.argv.push("--user".into());
    // `--install-deps-from` is the one part that *is* affected, and it is the
    // reason this function needs to know where Flathub is. flatpak-builder acts
    // on it by running `flatpak --user install -y --noninteractive flathub …`,
    // in the same installation as the build — so on a computer where Flathub is
    // set up system-wide, which is the Fedora default, the build dies with
    // "No remote refs found for ‘flathub’" *even though the runtime is already
    // installed and the build would otherwise have worked*. Leaving the flag off
    // is strictly better there: anything genuinely missing is caught by the
    // checklist before the build, where the download can be aimed at the right
    // installation.
    if options.fetch_missing && flathub == Scope::User {
        command.argv.push("--install-deps-from=flathub".into());
    }
    if options.install_after {
        command.argv.push("--install".into());
    }
    if options.keep_build_dirs {
        command.argv.push("--keep-build-dirs".into());
    }
    command.argv.push(BUILD_DIR.into());
    command.argv.push(manifest_file.into());
    command
}

pub fn install_command(app_id: &str) -> Command {
    let mut command = Command::new(&["flatpak-builder", "--user", "--install", "--force-clean"]);
    command.argv.push(BUILD_DIR.into());
    command.argv.push(format!("{app_id}.yml"));
    command
}

pub fn run_command(app_id: &str) -> Command {
    Command::new(&["flatpak", "run", app_id])
}

/// Exporting takes two steps: put the build into a repository, then wrap that
/// repository up as the single file people can pass around.
pub fn export_commands(app_id: &str, options: &Options, flathub: Scope) -> Vec<Command> {
    let options = Options {
        make_bundle: true,
        ..options.clone()
    };
    vec![
        build_command(&format!("{app_id}.yml"), &options, flathub),
        bundle_command(app_id),
    ]
}

/// Wrapping the repository the build wrote into the one file people can pass
/// around. Separate from the build because the build page runs the two as one
/// shell line and needs each half.
pub fn bundle_command(app_id: &str) -> Command {
    Command {
        argv: vec![
            "flatpak".into(),
            "build-bundle".into(),
            REPO_DIR.into(),
            format!("{app_id}.flatpak"),
            app_id.into(),
        ],
    }
}

/// The name of the file a build with `make_bundle` leaves behind.
pub fn bundle_file(app_id: &str) -> String {
    format!("{app_id}.flatpak")
}

/// Where a remote lives.
///
/// Flatpak keeps two installations — one for the whole computer and one for the
/// user — and a remote in one is invisible to a command aimed at the other. This
/// is not a detail that can be skipped: on Fedora, Flathub is set up
/// system-wide, so `flatpak install --user flathub org.gnome.Sdk//50` fails with
/// "No remote refs found for ‘flathub’" while every listing shows Flathub
/// present. That is a computer that is set up correctly being told it isn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Missing,
    User,
    System,
}

impl Scope {
    /// The flag that aims a command at this installation.
    fn flag(&self) -> &'static str {
        match self {
            // Nothing is going to work, but a command has to say something; the
            // check above this one is what deals with the remote being absent.
            Scope::Missing | Scope::User => "--user",
            Scope::System => "--system",
        }
    }

    pub fn is_present(&self) -> bool {
        !matches!(self, Scope::Missing)
    }
}

/// Which installation holds a remote, read from `flatpak remotes
/// --columns=name,options`. The options column is a comma-separated list with
/// `user` or `system` among it: `flathub  system,filtered`.
///
/// The user installation wins when a remote is in both, because that is where a
/// command with no flag would land for a user who has one.
pub fn remote_scope(remotes: &str, name: &str) -> Scope {
    let mut found = Scope::Missing;
    for line in remotes.lines() {
        let mut columns = line.split('\t');
        if columns.next().map(str::trim) != Some(name) {
            continue;
        }
        let options = columns.next().unwrap_or_default();
        let is_user = options.split(',').any(|option| option.trim() == "user");
        if is_user {
            return Scope::User;
        }
        found = Scope::System;
    }
    found
}

pub fn install_refs_command(refs: &[String], scope: Scope) -> Command {
    let mut command = Command::new(&["flatpak", "install", scope.flag(), "-y", "flathub"]);
    command.argv.extend(refs.iter().cloned());
    command
}

// -- installing the tools ----------------------------------------------------

/// The command that installs a package on *this* computer.
///
/// "Copy this and run it" is only helpful if the line works where it is pasted,
/// and `sudo dnf install …` on Debian is worse than no suggestion at all for
/// exactly the person this app is written for. The distribution is read out of
/// `os-release`; anything unrecognised gets no command, and the check says to
/// use the distribution's own package manager instead.
pub fn package_command(os_release: &str, package: &str) -> Option<Command> {
    let field = |key: &str| -> String {
        os_release
            .lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .map(|value| value.trim().trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };

    // ID_LIKE catches the derivatives — Linux Mint says `ID_LIKE=ubuntu debian`,
    // Nobara says `ID_LIKE=fedora` — which is most of what people actually run.
    let families = format!("{} {}", field("ID"), field("ID_LIKE"));
    let has = |name: &str| families.split_whitespace().any(|word| word == name);

    let argv: Vec<&str> = if has("fedora") || has("rhel") || has("centos") {
        vec!["sudo", "dnf", "install", package]
    } else if has("debian") || has("ubuntu") {
        vec!["sudo", "apt", "install", package]
    } else if has("arch") {
        vec!["sudo", "pacman", "-S", package]
    } else if has("opensuse") || has("suse") {
        vec!["sudo", "zypper", "install", package]
    } else if has("alpine") {
        vec!["sudo", "apk", "add", package]
    } else if has("gentoo") {
        vec!["sudo", "emerge", package]
    } else {
        return None;
    };

    Some(Command::new(&argv))
}

/// `os-release` as this computer has it. Inside a Flatpak the one at `/etc` is
/// the runtime's — Freedesktop, always — so the host's copy is the only one that
/// answers "what would install a package here".
pub fn host_os_release() -> String {
    for path in ["/run/host/os-release", "/etc/os-release"] {
        if let Ok(text) = std::fs::read_to_string(path) {
            return text;
        }
    }
    String::new()
}

/// The check offered when a package is missing: a command where one is known,
/// and the package's name where it isn't.
fn install_fix(package: &str) -> (String, Option<Fix>) {
    install_fix_for(&host_os_release(), package)
}

/// The half of [`install_fix`] that doesn't read the disk, so the rule can be
/// tested on every distribution rather than only on this one.
fn install_fix_for(os_release: &str, package: &str) -> (String, Option<Fix>) {
    match package_command(os_release, package) {
        Some(command) => (
            String::new(),
            Some(Fix::Copy {
                label: "Copy the command".into(),
                command,
            }),
        ),
        None => (
            format!(" Install the package your distribution calls “{package}”."),
            None,
        ),
    }
}

pub fn add_flathub_command() -> Command {
    Command::new(&[
        "flatpak",
        "remote-add",
        "--user",
        "--if-not-exists",
        "flathub",
        "https://dl.flathub.org/repo/flathub.flatpakrepo",
    ])
}

/// The runtime and SDK a manifest needs, in the `id//version` form flatpak wants.
pub fn required_refs(project: &Project) -> Vec<String> {
    let manifest = &project.manifest;
    let version = manifest.runtime_version.trim();
    let mut refs = Vec::new();

    for id in [manifest.runtime.trim(), manifest.sdk.trim()] {
        if !id.is_empty() && !version.is_empty() {
            refs.push(format!("{id}//{version}"));
        }
    }
    for extension in &manifest.sdk_extensions {
        // Extensions are versioned against the freedesktop base, not the app's
        // runtime, so the version is left off and flatpak resolves it.
        if !extension.trim().is_empty() {
            refs.push(extension.trim().to_string());
        }
    }
    refs
}

// -- running it in a terminal instead ----------------------------------------

/// How a terminal wants to be told to run something. They agree on almost
/// nothing, which is why this is a table rather than one command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalStyle {
    /// `--working-directory=DIR -- sh -lc …` (ptyxis, gnome-terminal, kgx)
    WorkingDirectoryThenDashDash,
    /// `--working-directory=DIR -e sh -lc …` (xfce4-terminal, mate-terminal)
    WorkingDirectoryThenDashE,
    /// `--workdir DIR -e sh -lc …` (konsole)
    WorkdirThenDashE,
    /// `--working-directory DIR -e sh -lc …` (alacritty, kitty, foot)
    SpacedWorkingDirectoryThenDashE,
    /// Nothing but the command; the script changes directory itself (xterm).
    JustTheCommand,
}

#[derive(Debug)]
pub struct Terminal {
    pub program: &'static str,
    /// What people call it, for "Open in Console".
    pub label: &'static str,
    pub style: TerminalStyle,
}

/// Tried in this order: the desktop's own terminal first, then the common ones.
pub const TERMINALS: &[Terminal] = &[
    Terminal {
        program: "ptyxis",
        label: "Terminal",
        style: TerminalStyle::WorkingDirectoryThenDashDash,
    },
    Terminal {
        program: "kgx",
        label: "Console",
        style: TerminalStyle::WorkingDirectoryThenDashDash,
    },
    Terminal {
        program: "gnome-terminal",
        label: "Terminal",
        style: TerminalStyle::WorkingDirectoryThenDashDash,
    },
    Terminal {
        program: "konsole",
        label: "Konsole",
        style: TerminalStyle::WorkdirThenDashE,
    },
    Terminal {
        program: "xfce4-terminal",
        label: "Terminal",
        style: TerminalStyle::WorkingDirectoryThenDashE,
    },
    Terminal {
        program: "alacritty",
        label: "Alacritty",
        style: TerminalStyle::SpacedWorkingDirectoryThenDashE,
    },
    Terminal {
        program: "kitty",
        label: "Kitty",
        style: TerminalStyle::SpacedWorkingDirectoryThenDashE,
    },
    Terminal {
        program: "foot",
        label: "Foot",
        style: TerminalStyle::SpacedWorkingDirectoryThenDashE,
    },
    Terminal {
        program: "xterm",
        label: "xterm",
        style: TerminalStyle::JustTheCommand,
    },
];

pub fn terminal(program: &str) -> Option<&'static Terminal> {
    TERMINALS.iter().find(|terminal| terminal.program == program)
}

/// The shell line the terminal runs: change to the project folder, run the
/// build, then wait. The waiting is the point — without it the window closes the
/// instant the build fails and takes the error with it.
pub fn keep_open_script(folder: &str, command: &str) -> String {
    format!(
        "cd {} && {command}; status=$?; echo; \
         if [ $status -eq 0 ]; then echo 'Finished.'; else echo \"Stopped with error $status.\"; fi; \
         echo 'This window stays open so you can read the log. Press Enter to close it.'; \
         read _",
        shell_quote(folder)
    )
}

/// The whole command line for opening a terminal on the build.
pub fn terminal_argv(terminal: &Terminal, folder: &str, command: &str) -> Vec<String> {
    let script = keep_open_script(folder, command);
    let shell = ["sh".to_string(), "-lc".to_string(), script];

    let mut argv = vec![terminal.program.to_string()];
    match terminal.style {
        TerminalStyle::WorkingDirectoryThenDashDash => {
            argv.push(format!("--working-directory={folder}"));
            argv.push("--".to_string());
        }
        TerminalStyle::WorkingDirectoryThenDashE => {
            argv.push(format!("--working-directory={folder}"));
            argv.push("-e".to_string());
        }
        TerminalStyle::WorkdirThenDashE => {
            argv.push("--workdir".to_string());
            argv.push(folder.to_string());
            argv.push("-e".to_string());
        }
        TerminalStyle::SpacedWorkingDirectoryThenDashE => {
            argv.push("--working-directory".to_string());
            argv.push(folder.to_string());
            argv.push("-e".to_string());
        }
        TerminalStyle::JustTheCommand => {
            argv.push("-e".to_string());
        }
    }
    argv.extend(shell);
    argv
}

/// Single quotes, the way a shell wants them, so a folder with a space or an
/// apostrophe in it stays one argument.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

// -- preflight ---------------------------------------------------------------

/// What the app found out about this computer. Gathered by the UI (it means
/// running things), judged here.
#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub flatpak_present: bool,
    pub builder_present: bool,
    /// Where Flathub is, if anywhere. A bool here was the bug: the check asked
    /// whether the remote existed at all, and the button then aimed at one
    /// installation in particular.
    pub flathub: Scope,
    /// Refs from [`required_refs`] that are already installed.
    pub installed_refs: Vec<String>,
    /// True when the app is sandboxed but cannot reach the host to build.
    pub sandboxed_without_host: bool,
    /// Whatever is wrong with the project. Kept as issues rather than sentences
    /// so the button that offers to fix them can go to the right field.
    pub blocking_issues: Vec<crate::validate::Issue>,
    /// Whether the manifest has been written to disk yet.
    pub manifest_written: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Ready,
    /// Something is missing, and the app can do something about it.
    Fixable,
    /// Something is missing and only the user can sort it out.
    Blocked,
}

/// What the app offers to do about a failed check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Run this, and say what it does.
    Run { label: String, command: Command },
    /// Nothing to press: the command is offered to copy instead.
    Copy { label: String, command: Command },
    /// Somewhere else in the app. The field says where, so pressing it can go
    /// there rather than describe the journey.
    Elsewhere {
        label: String,
        field: Option<crate::validate::Field>,
    },
}

#[derive(Debug, Clone)]
pub struct Check {
    pub title: String,
    pub detail: String,
    pub state: State,
    pub fix: Option<Fix>,
}

/// The checklist shown before the build button becomes live. Each line says what
/// is wrong in the user's terms and, where the app can help, how to fix it.
pub fn preflight(project: &Project, probe: &Probe) -> Vec<Check> {
    let mut checks = environment_checks(probe);
    checks.extend(project_checks(project, probe));
    checks
}

/// The parts that are about this computer rather than about any project. Shown
/// on the welcome page too, the first time the app is opened somewhere that
/// isn't set up yet — better to say so before someone has filled in seven steps.
pub fn environment_checks(probe: &Probe) -> Vec<Check> {
    let mut checks = Vec::new();

    checks.push(if probe.flatpak_present {
        Check {
            title: "Flatpak is installed".into(),
            detail: "The tool that runs and installs Flatpak apps.".into(),
            state: State::Ready,
            fix: None,
        }
    } else {
        let (extra, fix) = install_fix("flatpak");
        Check {
            title: "Flatpak isn't installed".into(),
            detail: format!(
                "Without it nothing here can be built or run. Your distribution \
                 packages it as “flatpak”.{extra}"
            ),
            state: State::Blocked,
            fix,
        }
    });

    checks.push(if probe.builder_present {
        Check {
            title: "flatpak-builder is installed".into(),
            detail: "The tool that turns a manifest into an app.".into(),
            state: State::Ready,
            fix: None,
        }
    } else {
        let (extra, fix) = install_fix("flatpak-builder");
        Check {
            title: "flatpak-builder isn't installed".into(),
            detail: format!(
                "This is the program that actually does the building. It is a \
                 separate package from Flatpak itself.{extra}"
            ),
            state: State::Blocked,
            fix,
        }
    });

    checks.push(if probe.flathub.is_present() {
        Check {
            title: "Flathub is set up".into(),
            detail: match probe.flathub {
                // Said out loud, because it decides what every download here is
                // going to ask for: a system install wants an administrator's
                // password, and being asked for one out of nowhere is exactly
                // the sort of surprise this app is meant to prevent.
                Scope::System => "Where the runtime and the build tools are downloaded \
                                  from. It is set up for the whole computer, so \
                                  downloading them asks for your password."
                    .into(),
                _ => "Where the runtime and the build tools are downloaded from.".into(),
            },
            state: State::Ready,
            fix: None,
        }
    } else {
        Check {
            title: "Flathub hasn't been added".into(),
            detail: "Flathub is where the runtime your app builds against comes from. \
                     Adding it is a one-off."
                .into(),
            state: State::Fixable,
            fix: Some(Fix::Run {
                label: "Add Flathub".into(),
                command: add_flathub_command(),
            }),
        }
    });

    checks
}

/// Everything else: what this particular project still needs.
fn project_checks(project: &Project, probe: &Probe) -> Vec<Check> {
    let mut checks = Vec::new();
    let required = required_refs(project);
    let missing: Vec<String> = required
        .iter()
        .filter(|needed| !probe.installed_refs.contains(needed))
        .cloned()
        .collect();

    checks.push(if required.is_empty() {
        Check {
            title: "No runtime chosen yet".into(),
            detail: "The build needs to know what the app runs on.".into(),
            state: State::Blocked,
            fix: Some(Fix::Elsewhere {
                label: "Choose a runtime".into(),
                field: Some(crate::validate::Field::Runtime),
            }),
        }
    } else if missing.is_empty() {
        Check {
            title: "The runtime and build tools are here".into(),
            detail: required.join(", "),
            state: State::Ready,
            fix: None,
        }
    } else {
        Check {
            title: format!(
                "{} still to download",
                if missing.len() == 1 {
                    "One thing".to_string()
                } else {
                    format!("{} things", missing.len())
                }
            ),
            detail: format!(
                "{}\n{}",
                missing.join(", "),
                "These are downloaded once and shared by every app that uses them. \
                 Expect a few hundred megabytes the first time."
            ),
            state: State::Fixable,
            fix: Some(Fix::Run {
                label: "Download them".into(),
                command: install_refs_command(&missing, probe.flathub),
            }),
        }
    });

    checks.push(if probe.manifest_written {
        Check {
            title: "The manifest is written".into(),
            detail: "The build reads it from the project folder.".into(),
            state: State::Ready,
            fix: None,
        }
    } else {
        Check {
            title: "The manifest hasn't been written yet".into(),
            detail: "The build works from the file on disk, not from what's on screen.".into(),
            state: State::Fixable,
            fix: Some(Fix::Elsewhere {
                label: "Write the files".into(),
                field: None,
            }),
        }
    });

    if !probe.blocking_issues.is_empty() {
        checks.push(Check {
            title: format!(
                "{} still to answer",
                if probe.blocking_issues.len() == 1 {
                    "One question".to_string()
                } else {
                    format!("{} questions", probe.blocking_issues.len())
                }
            ),
            detail: probe
                .blocking_issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            state: State::Blocked,
            fix: Some(Fix::Elsewhere {
                label: "Take me there".into(),
                field: probe.blocking_issues.first().map(|issue| issue.field),
            }),
        });
    }

    // A project whose dependencies were never written down builds for a while
    // and then fails on the first crate it can't find — "no matching package
    // named `dirs` found", which reads like the crate is missing rather than
    // like the list is. It is a warning while editing, because a project isn't
    // wrong for not having got there yet; on the way into a build it is the
    // difference between a build and a wasted one. (A real build hit this.)
    if let Some(folder) = project.source_dir.as_deref() {
        let kind = crate::detect::detect(folder).kind;
        for need in crate::vendor::needs(kind, Some(folder), &project.manifest) {
            if need.lock_path.is_none() || need.is_ready() {
                continue;
            }
            checks.push(Check {
                title: format!(
                    "The {} this project uses haven't been written down yet",
                    need.ecosystem.label().to_lowercase()
                ),
                detail: "A Flatpak build has no internet, so everything the build \
                         downloads has to be listed in the manifest first. Without that \
                         list the build starts, runs for a while, and stops at the first \
                         thing it cannot find."
                    .into(),
                state: State::Blocked,
                fix: Some(Fix::Elsewhere {
                    label: "Sort out the dependencies".into(),
                    field: Some(crate::validate::Field::Dependencies),
                }),
            });
        }
    }

    if probe.sandboxed_without_host {
        checks.push(Check {
            title: "This copy can't start a build".into(),
            detail: "Pack It Flat is itself running in a sandbox, and this one hasn't \
                     been given permission to run programs outside it. Everything else \
                     works: the files are written normally, and the command below builds \
                     them in a terminal."
                .into(),
            state: State::Blocked,
            fix: None,
        });
    }

    checks
}

/// Whether the build button should be live.
pub fn can_build(checks: &[Check]) -> bool {
    checks.iter().all(|check| check.state != State::Blocked)
}

// -- reading the log ---------------------------------------------------------

/// A failure, explained. `raw` is kept so the log can still be shown to someone
/// who wants it — behind an expander, never as the first thing they see.
#[derive(Debug, Clone)]
pub struct Diagnosis {
    pub headline: String,
    pub detail: String,
    pub fix: Option<Fix>,
    /// The handful of lines the diagnosis came from.
    pub excerpt: String,
}

/// What went wrong, from the log and the exit code. The patterns are the
/// failures beginners actually hit, in the order they are most likely.
pub fn diagnose(project: &Project, log: &str, exit_code: i32, flathub: Scope) -> Diagnosis {
    let app_id = project.manifest.app_id.trim().to_string();

    if exit_code == 0 {
        return Diagnosis {
            headline: "The build finished".into(),
            detail: "Nothing went wrong.".into(),
            fix: None,
            excerpt: tail(log, 10),
        };
    }

    // The manifest itself doesn't parse. This is first because it happens first:
    // flatpak-builder reads the file before it looks at a runtime, so nothing
    // below can be the real reason when this line is present. Written by this
    // app the file always parses — but it is a file on disk, and the raw pane,
    // another editor or a half-finished hand edit can all leave it broken, and
    // then flatpak-builder is the first thing to notice.
    //
    // The wording is flatpak-builder's own: `Can't parse 'app.yml': 4:1: did not
    // find expected ',' or ']'`. Matched with the quote so a compiler saying
    // something similar about its own input can't take it.
    if let Some(line) = find(log, &["can't parse '"]) {
        return Diagnosis {
            headline: "The manifest can't be read".into(),
            detail: "flatpak-builder couldn't make sense of the manifest file. That \
                     means it has been edited by hand since it was written — one wrong \
                     indent or a missing quote is enough. The numbers in the message \
                     are the line and column where it gave up."
                .into(),
            fix: Some(Fix::Elsewhere {
                label: "Open the manifest text".into(),
                field: None,
            }),
            excerpt: line,
        };
    }

    // The app's listing is refused. Early, because it is unmistakable and it
    // happens at the very end: everything compiled, everything installed, and
    // then the build stopped on a one-word code nobody could be expected to
    // read. flatpak-builder hands the installed metainfo file to `appstreamcli
    // compose`, which insists on a summary, a description and a category — all
    // three reproduced against flatpak-builder 1.4.10 and the GNOME 50 SDK, one
    // field at a time.
    if find(log, &["appstreamcli compose failed"]).is_some() {
        let (headline, detail, field, code) = if log.contains("metainfo-no-summary") {
            (
                "The app needs a short description",
                "Everything built. What stopped at the last moment is the app's \
                 listing — the part an app store shows — and it can't be made \
                 without one line saying what the app does.",
                Some(crate::validate::Field::Summary),
                "metainfo-no-summary",
            )
        } else if log.contains("description-missing") {
            (
                "The app needs a longer description",
                "Everything built. The app's listing can't be made without a few \
                 sentences describing the app — it is what someone reads before \
                 installing it.",
                Some(crate::validate::Field::Description),
                "description-missing",
            )
        } else if log.contains("no-valid-category") {
            (
                "The app needs a category",
                "Everything built. The app's listing can't be made until the app \
                 says where it belongs — a category is what files it in a menu and \
                 in a store.",
                Some(crate::validate::Field::Categories),
                "no-valid-category",
            )
        } else {
            (
                "The app's listing was refused",
                "Everything built. What failed is the app information file: the \
                 program that turns it into a store listing found something it \
                 won't accept. The code below names it.",
                None,
                "appstreamcli compose failed",
            )
        };
        return Diagnosis {
            headline: headline.into(),
            detail: detail.into(),
            fix: Some(Fix::Elsewhere {
                label: "Take me there".into(),
                field,
            }),
            excerpt: find(log, &[code]).unwrap_or_else(|| tail(log, 10)),
        };
    }

    // No build system named, so flatpak-builder used its default — autotools —
    // on a project that has never heard of it. The message names a file the
    // project was never going to have, and says nothing about the real cause.
    // Wording copied verbatim from flatpak-builder 1.4.10:
    //   Error: module selfclone: Can't find autogen, autogen.sh or bootstrap
    if let Some(line) = find(log, &["can't find autogen"]) {
        return Diagnosis {
            headline: "No build system was chosen".into(),
            detail: "With nothing chosen, flatpak-builder assumes the project is built \
                     the way old C programs are, and looks for a file called autogen.sh \
                     to start it off. Your project doesn't have one, and doesn't need \
                     one. Choosing how it is built — for Rust, Node and anything else \
                     that spells its build out, that is “Commands I write myself” — \
                     puts one line in the manifest and this goes away."
                .into(),
            fix: Some(Fix::Elsewhere {
                label: "Choose how it's built".into(),
                field: Some(crate::validate::Field::BuildSystem),
            }),
            excerpt: line,
        };
    }

    // Missing runtime or SDK.
    if let Some(line) = find(log, &["unable to find sdk", "unable to find runtime"]) {
        let refs = required_refs(project);
        return Diagnosis {
            headline: "The runtime this app builds on isn't installed".into(),
            detail: "A Flatpak is built against a runtime — a shared set of libraries — \
                     and against the matching SDK. Neither is on this computer yet. They \
                     are downloaded once and reused by every app."
                .into(),
            fix: (!refs.is_empty()).then(|| Fix::Run {
                label: "Download them".into(),
                command: install_refs_command(&refs, flathub),
            }),
            excerpt: line,
        };
    }

    if let Some(line) = find(
        log,
        &["no remote refs found", "remote \"flathub\"", "remote 'flathub'"],
    ) {
        return Diagnosis {
            headline: "Flathub hasn't been set up on this computer".into(),
            detail: "That is where the runtime and the build tools come from. Adding it \
                     is a one-off and doesn't change anything else."
                .into(),
            fix: Some(Fix::Run {
                label: "Add Flathub".into(),
                command: add_flathub_command(),
            }),
            excerpt: line,
        };
    }

    // The build compiled everything and then had nowhere to put it. This comes
    // before the download checks on purpose: a CMake project that fetches
    // anything prints a FetchContent warning in *every* build, including the
    // ones that get this far, and matching that first would blame the wrong
    // thing entirely.
    if let Some(line) = find(
        log,
        &[
            "unknown target 'install'",
            "unknown target \"install\"",
            "no rule to make target 'install'",
            "no rule to make target \"install\"",
        ],
    ) {
        let program = project.manifest.command.trim();
        let program = if program.is_empty() { "the program" } else { program };
        return Diagnosis {
            headline: "The build worked, but the project never says where the program goes"
                .into(),
            detail: format!(
                "Everything compiled. The last step of a Flatpak build is installing what \
                 was built into /app, and this project's own build files have no install \
                 rule, so there was nothing to run. It is a line missing from \
                 CMakeLists.txt or meson.build, not anything wrong with the manifest: \
                 {program} has to end up at /app/bin/, with the desktop entry and the \
                 metainfo beside it."
            ),
            fix: Some(Fix::Elsewhere {
                label: "See the lines to add".into(),
                field: Some(crate::validate::Field::BuildSystem),
            }),
            excerpt: line,
        };
    }

    // CMake fetching its own dependencies is the same wall, but it says so in
    // its own words and the fix is a different one — the app can't write a list
    // for C and C++, so the dependency has to become a source in the manifest.
    // The two module names only count on a line that also failed: CMake names
    // FetchContent.cmake in ordinary policy warnings too.
    if let Some(line) = find(
        log,
        &[
            "failed to download",
            "download failed",
            "error: downloading",
        ],
    )
    .or_else(|| find_failing(log, &["fetchcontent", "externalproject"]))
    {
        return Diagnosis {
            headline: "The build tried to download a dependency of its own".into(),
            detail: "CMake projects often fetch what they need while configuring, with \
                     FetchContent or ExternalProject. That works in a terminal and cannot \
                     work here: a Flatpak build has no internet, on purpose. Each of those \
                     dependencies has to be added to the manifest as a source — an archive \
                     with its address and checksum — and CMake told to use the copy \
                     instead of fetching it."
                .into(),
            fix: Some(Fix::Elsewhere {
                label: "Sort out the dependencies".into(),
                field: Some(crate::validate::Field::Dependencies),
            }),
            excerpt: line,
        };
    }

    // A build has no network. This is the failure that confuses people most,
    // because the same command works outside the sandbox.
    if let Some(line) = find(
        log,
        &[
            "could not resolve host",
            "network is unreachable",
            "temporary failure in name resolution",
            "failed to fetch",
            "no such host",
            "enotfound",
            "spurious network error",
            "unable to get packages",
        ],
    ) {
        return Diagnosis {
            headline: "The build tried to download something, and it can't".into(),
            detail: "A Flatpak build has no internet access on purpose: it is what makes \
                     a build repeatable, and stops a package changing under you. \
                     Everything the build needs has to be listed in the manifest first — \
                     for Rust that is a generated list of crates, for Node one of \
                     packages."
                .into(),
            fix: Some(Fix::Elsewhere {
                label: "Prepare the dependency list".into(),
                field: Some(crate::validate::Field::Dependencies),
            }),
            excerpt: line,
        };
    }

    if let Some(line) = find(log, &["wrong sha256", "checksum mismatch", "hash mismatch"]) {
        return Diagnosis {
            headline: "A downloaded file isn't the one the manifest expects".into(),
            detail: "The manifest pins each download to a checksum, so a file that has \
                     changed stops the build instead of being built silently. Either the \
                     file was re-released, or the checksum was typed wrongly."
                .into(),
            fix: Some(Fix::Elsewhere {
                label: "Check the sources".into(),
                field: Some(crate::validate::Field::Sources),
            }),
            excerpt: line,
        };
    }

    if !app_id.is_empty() {
        if let Some(line) = find(
            log,
            &[
                "does not match the app id",
                "wrong number of desktop files",
                "no desktop file",
                "appstream",
                "metainfo",
            ],
        ) {
            return Diagnosis {
                headline: "The app's files don't all agree on its name".into(),
                detail: format!(
                    "Flatpak wants the desktop entry, the app information and the icon \
                     all named after the app ID — {app_id}.desktop, \
                     {app_id}.metainfo.xml and {app_id}.svg. Writing the files from here \
                     names them for you."
                ),
                fix: Some(Fix::Elsewhere {
                    label: "Write the files again".into(),
                    field: None,
                }),
                excerpt: line,
            };
        }
    }

    if let Some(line) = find(log, &["no space left on device"]) {
        return Diagnosis {
            headline: "The disk is full".into(),
            detail: "A Flatpak build needs a few gigabytes of room for the runtime and \
                     the working files."
                .into(),
            fix: None,
            excerpt: line,
        };
    }

    if let Some(line) = find(log, &["permission denied"]) {
        return Diagnosis {
            headline: "Something in the project folder can't be read or written".into(),
            detail: "The build needs to read your code and write its working files beside \
                     it. A folder owned by another user, or left behind by an earlier \
                     build run as root, causes this."
                .into(),
            fix: None,
            excerpt: line,
        };
    }

    // "cargo: command not found" is not the same failure as flatpak-builder
    // being missing, and telling someone to install flatpak-builder when it was
    // the compiler that wasn't found sends them a long way in the wrong
    // direction. Which program it was decides the answer.
    if let Some(line) = find(log, &["command not found"]) {
        let missing = missing_tool(&line);

        if let Some(extension) = missing
            .as_deref()
            .and_then(crate::runtimes::extension_for_tool)
        {
            let switched_on = project
                .manifest
                .sdk_extensions
                .iter()
                .any(|id| id == extension.id);

            return Diagnosis {
                headline: format!(
                    "The build couldn't find {}.",
                    missing.clone().unwrap_or_else(|| extension.label.to_string())
                ),
                detail: if switched_on {
                    format!(
                        "The {} tools are switched on, so they are installed — but the \
                         manifest doesn't say where they are. A Flatpak build has its own \
                         world: the compiler lives at {}, and nothing looks there unless \
                         the manifest says append-path. Your computer's own {} is never \
                         used, which is what makes the build repeatable.",
                        extension.label,
                        extension.bin_path,
                        missing.clone().unwrap_or_default()
                    )
                } else {
                    format!(
                        "A Flatpak build has its own world, and {} isn't in it by \
                         default. Your computer's own copy is never used — that is what \
                         makes the build the same everywhere. Switch on “{}” and the \
                         compiler is added to the build, with the line that tells it \
                         where to look.",
                        missing.clone().unwrap_or_default(),
                        extension.label
                    )
                },
                fix: Some(Fix::Elsewhere {
                    label: format!("Set up {}", extension.label),
                    field: Some(crate::validate::Field::Runtime),
                }),
                excerpt: line,
            };
        }

        if line.to_lowercase().contains("flatpak-builder") || exit_code == 127 && log.trim().is_empty()
        {
            let (extra, fix) = install_fix("flatpak-builder");
            return Diagnosis {
                headline: "flatpak-builder isn't installed".into(),
                detail: format!(
                    "It is a separate package from Flatpak itself, and it is what \
                     actually does the building.{extra}"
                ),
                fix,
                excerpt: tail(log, 5),
            };
        }

        return Diagnosis {
            headline: format!(
                "The build couldn't find {}.",
                missing.unwrap_or_else(|| "a program it needs".into())
            ),
            detail: "A Flatpak build only has what the runtime, the SDK and the manifest \
                     provide — never what is installed on your computer. Whatever this \
                     program is, it has to come from an SDK extension or be built as \
                     another part of the app."
                .into(),
            fix: None,
            excerpt: line,
        };
    }

    // Anything else: the build's own last words, which are usually the compiler's.
    Diagnosis {
        headline: "The build stopped with an error".into(),
        detail: "This one isn't a Flatpak problem — it comes from the program being \
                 built. The last lines of the log are below; they usually name the file \
                 and line that failed."
            .into(),
        fix: None,
        excerpt: tail(log, 25),
    }
}

/// The program named in a "command not found" line. Shells write it as
/// `/bin/sh: line 1: cargo: command not found`, so it is the word before the
/// message.
fn missing_tool(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let at = lower.find("command not found")?;
    let before = line[..at].trim_end().trim_end_matches(':').trim();
    let name = before.rsplit([':', ' ']).next()?.trim();
    (!name.is_empty() && !name.chars().any(char::is_whitespace)).then(|| name.to_string())
}

/// The first line containing any of these, with a little of what follows it.
fn find(log: &str, needles: &[&str]) -> Option<String> {
    let lines: Vec<&str> = log.lines().collect();
    let index = lines.iter().position(|line| {
        let lower = line.to_lowercase();
        needles.iter().any(|needle| lower.contains(needle))
    })?;

    let end = (index + 3).min(lines.len());
    Some(lines[index..end].join("\n"))
}

/// Like `find`, but only on a line that actually failed. Some names appear in a
/// build that is going perfectly well — CMake mentions FetchContent.cmake in its
/// policy warnings — and reading one of those as the failure sends the user off
/// to fix something that isn't broken.
fn find_failing(log: &str, needles: &[&str]) -> Option<String> {
    let lines: Vec<&str> = log.lines().collect();
    let index = lines.iter().position(|line| {
        let lower = line.to_lowercase();
        (lower.contains("error") || lower.contains("failed"))
            && needles.iter().any(|needle| lower.contains(needle))
    })?;

    let end = (index + 3).min(lines.len());
    Some(lines[index..end].join("\n"))
}

fn tail(log: &str, lines: usize) -> String {
    let all: Vec<&str> = log.lines().filter(|line| !line.trim().is_empty()).collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

/// A line worth putting above the log while the build runs. flatpak-builder's
/// own output says what it is doing; this turns that into a sentence.
pub fn status_line(log_line: &str) -> Option<String> {
    let lower = log_line.to_lowercase();
    let phase = if lower.starts_with("downloading") || lower.contains("fetching") {
        "Downloading what the build needs — this depends on your connection"
    // flatpak-builder prints "Running: <command>" for every line in
    // build-commands, which in a real build is most of what scrolls past while
    // the module is being built. Only `make` was matched, so a simple module —
    // the kind this app writes — left the banner saying whatever came before.
    } else if lower.contains("building module") || lower.starts_with("running:") {
        "Building — this can take several minutes, and the log below is normal"
    } else if lower.contains("compiling ") {
        "Compiling — this is the slow part"
    // "Finishing app" is the phase between the build and the export, and it was
    // the one gap left in a real log from end to end.
    } else if lower.contains("exporting")
        || lower.contains("committing")
        || lower.starts_with("finishing")
    {
        "Nearly there: packaging what was built"
    } else if lower.contains("pruning") || lower.contains("cleaning") {
        "Tidying up"
    } else {
        return None;
    };
    Some(phase.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    fn project() -> Project {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.Sample\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: sample\n\
             sdk-extensions:\n  - org.freedesktop.Sdk.Extension.rust-stable\n",
        )
        .unwrap();
        Project::from_import(&import, None)
    }

    #[test]
    fn the_build_command_matches_what_the_documentation_tells_people_to_type() {
        // Without the bundle step, which is the app's own addition: this is the
        // line the generated manifest's header tells people to type, and it is
        // deliberately the form that works on any computer — no
        // `--install-deps-from`, because where that looks for Flathub depends on
        // the installation and getting it wrong kills a build that would have
        // worked.
        let options = Options {
            make_bundle: false,
            ..Options::default()
        };
        assert_eq!(
            build_command("no.oyzmo.Sample.yml", &options, Scope::System).as_typed(),
            "flatpak-builder --force-clean --user build-dir no.oyzmo.Sample.yml"
        );
    }

    /// Pressing "Build" has to leave a file behind. A build that produces only a
    /// `build-dir` is one nobody asked for, so the repository the bundle is made
    /// from has to be written by the build itself — it cannot be added after.
    #[test]
    fn a_build_leaves_a_flatpak_file_behind_by_default() {
        assert!(Options::default().make_bundle);

        let command = build_command("no.oyzmo.Sample.yml", &Options::default(), Scope::User);
        assert!(
            command.argv.contains(&format!("--repo={REPO_DIR}")),
            "{command:?}"
        );

        let bundle = bundle_command("no.oyzmo.Sample");
        assert_eq!(
            bundle.as_typed(),
            "flatpak build-bundle build-repo no.oyzmo.Sample.flatpak no.oyzmo.Sample"
        );
        assert_eq!(bundle_file("no.oyzmo.Sample"), "no.oyzmo.Sample.flatpak");

        // Exporting on its own still works for a build that was made without it.
        let commands = export_commands(
            "no.oyzmo.Sample",
            &Options {
                make_bundle: false,
                ..Options::default()
            },
            Scope::User,
        );
        assert_eq!(commands.len(), 2);
        assert!(commands[0].argv.contains(&format!("--repo={REPO_DIR}")));
        assert_eq!(commands[1], bundle);
    }

    #[test]
    fn each_switch_adds_exactly_one_thing() {
        let mut options = Options {
            clean_first: false,
            fetch_missing: false,
            install_after: false,
            keep_build_dirs: false,
            make_bundle: false,
        };
        assert_eq!(
            build_command("m.yml", &options, Scope::User).as_typed(),
            "flatpak-builder --user build-dir m.yml"
        );

        options.install_after = true;
        assert!(build_command("m.yml", &options, Scope::User)
            .argv
            .contains(&"--install".to_string()));

        options.keep_build_dirs = true;
        assert!(build_command("m.yml", &options, Scope::User)
            .argv
            .contains(&"--keep-build-dirs".to_string()));

        for option in Option_::ALL {
            assert!(!option.label().is_empty());
            assert!(option.explanation().ends_with('.'), "{option:?}");
        }
    }

    #[test]
    fn inside_the_sandbox_the_build_runs_on_the_host() {
        let command = build_command("m.yml", &Options::default(), Scope::User);
        assert_eq!(command.on_host(false), command);

        let host = command.on_host(true);
        assert_eq!(host.argv[0], "flatpak-spawn");
        assert_eq!(host.argv[1], "--host");
        assert!(host.argv.contains(&"flatpak-builder".to_string()));
    }

    #[test]
    fn the_runtime_the_sdk_and_the_extensions_are_all_required() {
        let refs = required_refs(&project());
        assert_eq!(
            refs,
            vec![
                "org.gnome.Platform//50",
                "org.gnome.Sdk//50",
                "org.freedesktop.Sdk.Extension.rust-stable"
            ]
        );
        assert!(install_refs_command(&refs, Scope::User)
            .as_typed()
            .starts_with("flatpak install --user -y flathub org.gnome.Platform//50"));
    }

    #[test]
    fn exporting_builds_into_a_repository_and_then_bundles_it() {
        let commands = export_commands("no.oyzmo.Sample", &Options::default(), Scope::User);
        assert!(commands[0].as_typed().contains("--repo=build-repo"));
        assert_eq!(
            commands[1].as_typed(),
            "flatpak build-bundle build-repo no.oyzmo.Sample.flatpak no.oyzmo.Sample"
        );
    }

    /// The failure a real build hit: the Rust extension installed and switched
    /// on, but no `append-path`, so the compiler was never found. Blaming
    /// flatpak-builder for this — as this once did — sends someone a very long
    /// way in the wrong direction.
    #[test]
    fn a_missing_compiler_is_not_blamed_on_flatpak_builder() {
        let log = "Dependency Extension: org.freedesktop.Sdk.Extension.rust-stable 25.08\n\
                   Building module namp in /home/me/.flatpak-builder/build/namp-1\n\
                   Running: cargo --offline fetch --manifest-path Cargo.toml --verbose\n\
                   /bin/sh: line 1: cargo: command not found\n\
                   Error: module namp: Child process exited with code 127\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(diagnosis.headline, "The build couldn't find cargo.");
        assert!(!diagnosis.detail.contains("flatpak-builder"));
        // The project has rust-stable switched on, so the answer is the path.
        assert!(diagnosis.detail.contains("/usr/lib/sdk/rust-stable/bin"));
        assert!(diagnosis.detail.contains("append-path"));
        assert!(diagnosis.excerpt.contains("cargo: command not found"));
    }

    #[test]
    fn a_compiler_that_was_never_switched_on_is_a_different_sentence() {
        let import = manifest::parse_str("app-id: no.oyzmo.Sample\ncommand: sample\n").unwrap();
        let bare = Project::from_import(&import, None);

        let diagnosis = diagnose(&bare, "/bin/sh: line 1: node: command not found\n", 1, Scope::User);
        assert_eq!(diagnosis.headline, "The build couldn't find node.");
        assert!(diagnosis.detail.contains("Node.js 22"));
        assert!(matches!(diagnosis.fix, Some(Fix::Elsewhere { .. })));
    }

    #[test]
    fn flatpak_builder_itself_missing_still_says_so() {
        let diagnosis = diagnose(
            &project(),
            "sh: line 1: flatpak-builder: command not found\n",
            127,
            Scope::User,
        );
        assert_eq!(diagnosis.headline, "flatpak-builder isn't installed");
        assert!(matches!(diagnosis.fix, Some(Fix::Copy { .. })));
    }

    #[test]
    fn the_missing_program_is_read_out_of_the_shells_own_wording() {
        assert_eq!(
            missing_tool("/bin/sh: line 1: cargo: command not found").as_deref(),
            Some("cargo")
        );
        assert_eq!(
            missing_tool("sh: npm: command not found").as_deref(),
            Some("npm")
        );
        assert_eq!(missing_tool("nothing to see here"), None);
    }

    #[test]
    fn each_terminal_is_told_the_way_it_wants_to_be() {
        let command = "flatpak-builder --user build-dir app.yml";

        let ptyxis = terminal_argv(terminal("ptyxis").unwrap(), "/home/me/app", command);
        assert_eq!(ptyxis[0], "ptyxis");
        assert_eq!(ptyxis[1], "--working-directory=/home/me/app");
        assert_eq!(ptyxis[2], "--");
        assert_eq!(ptyxis[3], "sh");

        let konsole = terminal_argv(terminal("konsole").unwrap(), "/home/me/app", command);
        assert_eq!(konsole[1..4], ["--workdir", "/home/me/app", "-e"]);

        let xterm = terminal_argv(terminal("xterm").unwrap(), "/home/me/app", command);
        assert_eq!(xterm[1], "-e");
        // xterm is told nothing about the folder, so the script has to do it.
        assert!(xterm.last().unwrap().starts_with("cd '/home/me/app'"));
    }

    #[test]
    fn the_terminal_window_stays_open_after_the_build() {
        let script = keep_open_script("/home/me/app", "flatpak-builder x y");
        assert!(script.contains("cd '/home/me/app' && flatpak-builder x y"));
        // Both outcomes are reported, and neither closes the window.
        assert!(script.contains("Finished."));
        assert!(script.contains("Stopped with error"));
        assert!(script.trim_end().ends_with("read _"));
    }

    #[test]
    fn a_folder_with_an_apostrophe_survives_the_shell() {
        let script = keep_open_script("/home/me/tom's code", "true");
        assert!(script.contains(r#"cd '/home/me/tom'\''s code'"#), "{script}");
    }

    #[test]
    fn every_terminal_in_the_list_is_named_for_a_person() {
        for terminal in TERMINALS {
            assert!(!terminal.label.is_empty(), "{}", terminal.program);
            let argv = terminal_argv(terminal, "/tmp/x", "true");
            assert_eq!(argv[0], terminal.program);
            assert!(argv.contains(&"sh".to_string()));
        }
        assert!(terminal("nonsense").is_none());
    }

    /// The commonest blocking issue, as validation produces it.
    fn missing_app_id() -> crate::validate::Issue {
        crate::validate::app_id("")
            .into_iter()
            .next()
            .expect("an empty app ID is an error")
    }

    fn ready_probe() -> Probe {
        Probe {
            flatpak_present: true,
            builder_present: true,
            flathub: Scope::User,
            installed_refs: required_refs(&project()),
            sandboxed_without_host: false,
            blocking_issues: Vec::new(),
            manifest_written: true,
        }
    }

    #[test]
    fn a_ready_computer_passes_every_check() {
        let checks = preflight(&project(), &ready_probe());
        assert!(checks.iter().all(|check| check.state == State::Ready), "{checks:#?}");
        assert!(can_build(&checks));
    }

    /// A Rust project whose crate list was never prepared cannot build, and
    /// finding that out twenty minutes in is the failure this whole page exists
    /// to prevent.
    #[test]
    fn a_project_whose_dependencies_were_never_written_down_cannot_be_built() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.lock"),
            "[[package]]\nname = \"dirs\"\nversion = \"5.0.1\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
             checksum = \"44c45a9d03d6676652bcb5e724c7e988de1acad23a711b5217ab9cbecbec2225\"\n",
        )
        .unwrap();

        let mut project = project();
        project.source_dir = Some(dir.path().to_path_buf());
        project.manifest.ensure_main_module("sample");

        let checks = preflight(&project, &ready_probe());
        let check = checks
            .iter()
            .find(|check| check.title.contains("written down"))
            .expect("a check about the dependencies");
        assert_eq!(check.state, State::Blocked);
        assert!(!can_build(&checks));
        assert!(matches!(
            check.fix,
            Some(Fix::Elsewhere {
                field: Some(crate::validate::Field::Dependencies),
                ..
            })
        ));

        // Once the list is in the manifest, the way out is not still blocked.
        crate::vendor::wire_in(&mut project.manifest, crate::vendor::Ecosystem::Cargo);
        std::fs::write(dir.path().join("cargo-sources.json"), "[]").unwrap();
        let after = preflight(&project, &ready_probe());
        assert!(can_build(&after), "{after:#?}");
    }

    #[test]
    fn a_missing_runtime_is_something_the_app_offers_to_fetch() {
        let mut probe = ready_probe();
        probe.installed_refs = vec!["org.gnome.Platform//50".into()];

        let checks = preflight(&project(), &probe);
        let check = checks
            .iter()
            .find(|check| check.state == State::Fixable)
            .expect("a fixable check");
        assert!(check.title.contains("2 things still to download"));

        match check.fix.as_ref().unwrap() {
            Fix::Run { command, .. } => {
                assert!(command.as_typed().contains("org.gnome.Sdk//50"));
                assert!(!command.as_typed().contains("org.gnome.Platform//50"));
            }
            other => panic!("expected a runnable fix, got {other:?}"),
        }
        // Fixable, so the build button stays live: the build itself can fetch it.
        assert!(can_build(&checks));
    }

    #[test]
    fn the_first_run_check_is_about_the_computer_only() {
        let mut probe = ready_probe();
        probe.flathub = Scope::Missing;
        // Nothing about a project: no manifest, no runtime, no questions.
        probe.manifest_written = false;
        probe.blocking_issues = vec![missing_app_id()];

        let checks = environment_checks(&probe);
        assert_eq!(checks.len(), 3);
        assert!(checks
            .iter()
            .all(|check| !check.title.contains("manifest") && !check.title.contains("question")));

        let flathub = checks.last().unwrap();
        assert_eq!(flathub.state, State::Fixable);
        assert!(matches!(flathub.fix, Some(Fix::Run { .. })));
    }

    #[test]
    fn a_missing_builder_blocks_and_offers_the_command_to_copy() {
        let mut probe = ready_probe();
        probe.builder_present = false;

        let checks = preflight(&project(), &probe);
        assert!(!can_build(&checks));
        let check = checks
            .iter()
            .find(|check| check.title.contains("flatpak-builder isn't"))
            .unwrap();
        assert!(matches!(check.fix, Some(Fix::Copy { .. })));
    }

    #[test]
    fn a_sandbox_that_cannot_reach_the_host_says_so_plainly() {
        let mut probe = ready_probe();
        probe.sandboxed_without_host = true;

        let checks = preflight(&project(), &probe);
        assert!(!can_build(&checks));
        let check = checks.last().unwrap();
        assert!(check.detail.contains("Everything else"));
        assert!(check.fix.is_none(), "there is nothing to press, and it says so");
    }

    #[test]
    fn unanswered_questions_block_the_build_and_name_themselves() {
        let mut probe = ready_probe();
        probe.blocking_issues = vec![missing_app_id()];

        let checks = preflight(&project(), &probe);
        assert!(!can_build(&checks));
        assert!(checks
            .iter()
            .any(|check| check.detail.contains("Every Flatpak needs an app ID.")));
    }

    #[test]
    fn a_missing_runtime_in_the_log_becomes_a_button() {
        let log = "Emitting manifest\n\
                   error: Failed to init: Unable to find sdk org.gnome.Sdk version 50\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(
            diagnosis.headline,
            "The runtime this app builds on isn't installed"
        );
        assert!(diagnosis.detail.contains("downloaded once"));
        match diagnosis.fix.unwrap() {
            Fix::Run { command, .. } => assert!(command.as_typed().contains("org.gnome.Sdk//50")),
            other => panic!("expected a runnable fix, got {other:?}"),
        }
        assert!(diagnosis.excerpt.contains("Unable to find sdk"));
    }

    /// The failure this rule exists for, reproduced against flatpak-builder
    /// 1.4.10 on Fedora 44 with the SDK already installed system-wide:
    ///
    /// ```text
    /// Dependency Sdk: org.gnome.Sdk 50
    /// Installing org.gnome.Sdk/x86_64/50 from flathub
    /// error: No remote refs found for ‘flathub’
    /// Error installing deps: running `flatpak --user install -y --noninteractive flathub org.gnome.Sdk/x86_64/50`
    /// ```
    ///
    /// flatpak-builder acts on `--install-deps-from` inside the installation the
    /// build is in, so pairing it with `--user` on a computer whose Flathub is
    /// system-wide kills a build that would otherwise have worked — the SDK was
    /// right there. Dropping the flag, the same build ran.
    #[test]
    fn a_system_wide_flathub_never_gets_install_deps_from() {
        let options = Options {
            fetch_missing: true,
            ..Options::default()
        };

        let system = build_command("m.yml", &options, Scope::System);
        assert!(
            !system.as_typed().contains("--install-deps-from"),
            "{}",
            system.as_typed()
        );
        // Still the user's installation to build into: that part needs no
        // password, and a system-wide runtime is found from it regardless.
        assert!(system.argv.contains(&"--user".to_string()));

        let user = build_command("m.yml", &options, Scope::User);
        assert!(user.as_typed().contains("--install-deps-from=flathub"));

        // Nothing to fetch from, nothing to fetch with.
        let missing = build_command("m.yml", &options, Scope::Missing);
        assert!(!missing.as_typed().contains("--install-deps-from"));
    }

    /// The output is this machine's, from `flatpak remotes --columns=name,options`
    /// on Fedora 44: Flathub is set up for the whole computer and there is no
    /// user installation at all. The old check asked only whether a remote
    /// called flathub existed anywhere, ticked "Flathub is set up", and then
    /// offered a download with `--user` on it — which fails with "No remote refs
    /// found for ‘flathub’" on a computer that is set up perfectly well.
    #[test]
    fn a_system_wide_flathub_is_not_downloaded_from_with_user() {
        let remotes = "fedora\tsystem,oci\nflathub\tsystem,filtered\n";
        assert_eq!(remote_scope(remotes, "flathub"), Scope::System);

        let refs = vec!["org.gnome.Sdk//50".to_string()];
        assert_eq!(
            install_refs_command(&refs, remote_scope(remotes, "flathub")).as_typed(),
            "flatpak install --system -y flathub org.gnome.Sdk//50"
        );
    }

    #[test]
    fn a_flathub_the_user_has_is_downloaded_from_without_a_password() {
        let remotes = "flathub\tuser\n";
        assert_eq!(remote_scope(remotes, "flathub"), Scope::User);
        assert!(install_refs_command(&[], Scope::User)
            .as_typed()
            .contains("--user"));

        // In both installations: the user's is where a plain command would land.
        let both = "flathub\tsystem,filtered\nflathub\tuser\n";
        assert_eq!(remote_scope(both, "flathub"), Scope::User);
    }

    #[test]
    fn no_flathub_anywhere_is_still_missing() {
        assert_eq!(remote_scope("fedora\tsystem,oci\n", "flathub"), Scope::Missing);
        assert_eq!(remote_scope("", "flathub"), Scope::Missing);
        assert!(!Scope::Missing.is_present());

        // And that is the case the "Add Flathub" button is for.
        let mut probe = ready_probe();
        probe.flathub = Scope::Missing;
        let checks = environment_checks(&probe);
        assert!(checks
            .iter()
            .any(|check| check.title == "Flathub hasn't been added"));
    }

    /// A system-wide Flathub means a password prompt, and being asked for one
    /// out of nowhere is the sort of surprise this app exists to prevent.
    #[test]
    fn a_system_wide_flathub_says_a_password_will_be_asked_for() {
        let mut probe = ready_probe();
        probe.flathub = Scope::System;
        let checks = environment_checks(&probe);
        let flathub = checks
            .iter()
            .find(|check| check.title == "Flathub is set up")
            .expect("the check is there");
        assert!(flathub.detail.contains("password"), "{}", flathub.detail);
    }

    /// Copied verbatim out of flatpak-builder 1.4.10, from a manifest with an
    /// unclosed bracket in it. Before this the whole message fell through to
    /// "the build stopped with an error" plus the log — for a failure the app
    /// can name exactly, and send the user to the one pane that can fix it.
    #[test]
    fn a_manifest_that_doesnt_parse_is_named_rather_than_quoted() {
        let log = "Can't parse 'bad.yml': 4:1: did not find expected ',' or ']'\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(diagnosis.headline, "The manifest can't be read");
        assert!(diagnosis.detail.contains("edited by hand"));
        assert!(matches!(diagnosis.fix, Some(Fix::Elsewhere { .. })));
        assert!(diagnosis.excerpt.contains("did not find expected"));
    }

    /// A compiler complaining about parsing its own input is not this.
    #[test]
    fn parsing_a_source_file_is_not_the_manifest_failing() {
        let log = "error: cannot parse the expression at src/main.rs:4\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);
        assert_ne!(diagnosis.headline, "The manifest can't be read");
    }

    /// The same failure, but copied verbatim out of flatpak-builder 1.4.10
    /// rather than written from memory. The hand-written version above is one
    /// line; the real thing leads with a different one, and `diagnose` reads
    /// from the top — so a rule that only ever saw the tidied version could be
    /// matching a line the tool never prints first.
    #[test]
    fn the_real_tools_missing_sdk_output_is_diagnosed_too() {
        let log = "error: org.gnome.Sdk/x86_64/50 not installed\n\
                   Failed to init: Unable to find sdk org.gnome.Sdk version 50\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(
            diagnosis.headline,
            "The runtime this app builds on isn't installed"
        );
        match diagnosis.fix.unwrap() {
            Fix::Run { command, .. } => assert!(command.as_typed().contains("org.gnome.Sdk//50")),
            other => panic!("expected a runnable fix, got {other:?}"),
        }
    }

    /// Copied verbatim out of flatpak-builder 1.4.10 (colour codes stripped),
    /// once per field: a toy project was built with a summary missing, then a
    /// category, then a description, and each time the whole build failed at the
    /// finish stage on a one-word code. The last line is all a beginner sees,
    /// and it names a program they have never heard of.
    /// Copied verbatim out of flatpak-builder 1.4.10, from a real build of this
    /// app's own repository against a manifest this app wrote. The manifest had
    /// the commands to build it and no `buildsystem:` line, so flatpak-builder
    /// used its default — autotools — and reported a missing file the project
    /// was never going to have.
    #[test]
    fn a_missing_build_system_is_named_rather_than_the_file_it_looked_for() {
        let log = "Running git lfs checkout\n\
                   Error: module selfclone: Can't find autogen, autogen.sh or bootstrap\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(diagnosis.headline, "No build system was chosen");
        assert!(diagnosis.detail.contains("autogen.sh"), "{}", diagnosis.detail);
        match diagnosis.fix.unwrap() {
            Fix::Elsewhere { field, .. } => {
                assert_eq!(field, Some(crate::validate::Field::BuildSystem))
            }
            other => panic!("expected somewhere to go, got {other:?}"),
        }
    }

    #[test]
    fn an_app_listing_the_build_refuses_names_the_field_rather_than_the_tool() {
        let log = "Committing stage finish to cache\n\
                   Errors were raised during this compose run:\n\
                   general\n  E: filters-but-no-output\n\n\
                   no.oyzmo.Hello\n  E: description-missing\n\
                   Refer to the generated issue report data for details on the individual problems.\n\
                   Error: ERROR: appstreamcli compose failed: Child process exited with code 1\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert_eq!(diagnosis.headline, "The app needs a longer description");
        assert!(diagnosis.detail.contains("Everything built"));
        match diagnosis.fix.unwrap() {
            Fix::Elsewhere { field, .. } => {
                assert_eq!(field, Some(crate::validate::Field::Description))
            }
            other => panic!("expected somewhere to go, got {other:?}"),
        }
        assert!(diagnosis.excerpt.contains("description-missing"));
    }

    #[test]
    fn the_other_two_listing_refusals_name_their_own_field() {
        for (code, headline, field) in [
            (
                "metainfo-no-summary",
                "The app needs a short description",
                crate::validate::Field::Summary,
            ),
            (
                "no-valid-category",
                "The app needs a category",
                crate::validate::Field::Categories,
            ),
        ] {
            let log = format!(
                "no.oyzmo.Hello\n  E: {code}\n\
                 Error: ERROR: appstreamcli compose failed: Child process exited with code 1\n"
            );
            let diagnosis = diagnose(&project(), &log, 1, Scope::User);
            assert_eq!(diagnosis.headline, headline, "{code}");
            match diagnosis.fix.unwrap() {
                Fix::Elsewhere { field: got, .. } => assert_eq!(got, Some(field), "{code}"),
                other => panic!("{code}: expected somewhere to go, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_build_reaching_for_the_network_is_explained_rather_than_quoted() {
        let log = "Building module cleaner\n\
                   warning: spurious network error (3 tries remaining)\n\
                   error: failed to get `serde` as a dependency\n\
                   Caused by: Could not resolve host: static.crates.io\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert!(diagnosis.headline.contains("tried to download something"));
        assert!(diagnosis.detail.contains("no internet access on purpose"));
        assert!(matches!(diagnosis.fix, Some(Fix::Elsewhere { .. })));
    }

    /// Also from a real attempt: everything compiled, and the build died on the
    /// last line because CMakeLists.txt has no `install()` rule. The log carries
    /// CMake's routine FetchContent policy warning too, and that must not be
    /// read as the failure — the fix for it would be the wrong fix.
    #[test]
    fn a_project_with_no_install_rule_is_told_so_and_not_blamed_on_downloads() {
        let log = "CMake Warning (author) at /usr/share/cmake-4.4/Modules/FetchContent.cmake:1386 (message):\n\
                   \x20 The DOWNLOAD_EXTRACT_TIMESTAMP option was not given and policy CMP0135\n\
                   Call Stack (most recent call first):\n\
                   \x20 CMakeLists.txt:15 (FetchContent_Declare)\n\
                   -- Build files have been written to: /run/build/cleaner\n\
                   [6/6] Linking CXX executable cleaner\n\
                   ninja: error: unknown target 'install'\n\
                   Error: module cleaner: Child process exited with code 1\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert!(diagnosis.headline.contains("never says where the program goes"));
        assert!(!diagnosis.detail.contains("no internet"));
        assert!(diagnosis.detail.contains("/app/bin/"));
        assert!(diagnosis.excerpt.contains("unknown target 'install'"));
        match diagnosis.fix {
            Some(Fix::Elsewhere { field, .. }) => {
                assert_eq!(field, Some(crate::validate::Field::BuildSystem))
            }
            other => panic!("expected somewhere to go, got {other:?}"),
        }
    }

    /// A FetchContent policy warning on its own is not a failure. Before the
    /// order was settled, this log was diagnosed as a blocked download.
    #[test]
    fn cmakes_routine_fetchcontent_warning_is_not_read_as_the_failure() {
        let log = "CMake Warning (author) at Modules/FetchContent.cmake:1386 (message):\n\
                   \x20 The DOWNLOAD_EXTRACT_TIMESTAMP option was not given\n\
                   \x20 CMakeLists.txt:15 (FetchContent_Declare)\n\
                   main.cpp:42:9: error: 'stoi' was not declared in this scope\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert!(!diagnosis.headline.contains("download a dependency of its own"));
    }

    /// From a real attempt at a C++ project: the log is mostly CMake noise, and
    /// the failure underneath is the same "no network" wall with a different fix.
    #[test]
    fn cmake_fetching_its_own_dependencies_is_recognised_as_such() {
        let log = "-- Found CURL: /usr/lib/cmake/CURL/CURLConfig.cmake (found version \"8.21.0\")\n\
                   CMake Warning (author) at Modules/FetchContent.cmake:1386 (message):\n\
                   \x20 The DOWNLOAD_EXTRACT_TIMESTAMP option was not given\n\
                   CMake Error at build/_deps/fmt-subbuild/CMakeLists.txt:16 (message):\n\
                   \x20 error: downloading 'https://github.com/fmtlib/fmt/archive/10.2.1.tar.gz' failed\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);

        assert!(diagnosis.headline.contains("download a dependency of its own"));
        assert!(diagnosis.detail.contains("FetchContent"));
        assert!(diagnosis.detail.contains("no internet"));
        match diagnosis.fix {
            Some(Fix::Elsewhere { field, .. }) => {
                assert_eq!(field, Some(crate::validate::Field::Dependencies))
            }
            other => panic!("expected somewhere to go, got {other:?}"),
        }
    }

    #[test]
    fn a_changed_download_is_named_as_a_checksum_problem() {
        let log = "Downloading project-1.2.0.tar.xz\n\
                   error: Wrong sha256 for project-1.2.0.tar.xz, expected abc123, was def456\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);
        assert!(diagnosis.headline.contains("isn't the one the manifest expects"));
        assert!(diagnosis.excerpt.contains("Wrong sha256"));
    }

    #[test]
    fn missing_flathub_is_recognised_from_the_log_too() {
        let log = "error: No remote refs found for ‘flathub’\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);
        assert!(diagnosis.headline.contains("Flathub"));
        assert!(matches!(diagnosis.fix, Some(Fix::Run { .. })));
    }

    #[test]
    fn a_filename_mismatch_points_back_at_the_generated_files() {
        let log = "appstream-util: no desktop file found for no.oyzmo.Sample\n";
        let diagnosis = diagnose(&project(), log, 1, Scope::User);
        assert!(diagnosis.headline.contains("don't all agree on its name"));
        assert!(diagnosis.detail.contains("no.oyzmo.Sample.desktop"));
    }

    #[test]
    fn anything_unrecognised_still_gets_a_sentence_and_the_last_lines() {
        let log = (1..40)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\nsrc/main.rs:14:9: error: expected `;`\n";
        let diagnosis = diagnose(&project(), &log, 101, Scope::User);

        assert_eq!(diagnosis.headline, "The build stopped with an error");
        assert!(diagnosis.detail.contains("comes from the program being built"));
        assert!(diagnosis.excerpt.contains("expected `;`"));
        assert!(diagnosis.excerpt.lines().count() <= 25);
        assert!(diagnosis.fix.is_none());
    }

    #[test]
    fn a_successful_build_is_not_diagnosed_as_a_failure() {
        let diagnosis = diagnose(&project(), "Exporting no.oyzmo.Sample\n", 0, Scope::User);
        assert_eq!(diagnosis.headline, "The build finished");
        assert!(diagnosis.fix.is_none());
    }

    #[test]
    fn the_log_is_turned_into_a_line_about_what_is_happening() {
        assert!(status_line("Downloading sources").unwrap().contains("Downloading"));
        assert!(status_line("Building module cleaner")
            .unwrap()
            .contains("several minutes"));
        assert!(status_line("   Compiling serde v1.0.0")
            .unwrap()
            .contains("slow part"));
        assert!(status_line("Exporting no.oyzmo.Sample").unwrap().contains("packaging"));
        assert!(status_line("some ordinary line").is_none());
    }

    /// Every phase line a real build printed, from a run that went all the way
    /// to a `.flatpak` file. Two of them used to fall through and leave the
    /// banner saying whatever it had said before — for most of the build, in the
    /// case of a `simple` module, since "Running:" is what each of its
    /// build-commands prints.
    #[test]
    fn every_phase_of_a_real_build_says_something() {
        for line in [
            "Downloading sources",
            "Building module hello",
            "Running: install -Dm755 hello.sh /app/bin/hello",
            "Committing stage build-hello to cache",
            "Cleaning up",
            "Finishing app",
            "Exporting share/applications/no.oyzmo.Hello.desktop",
            "Pruning cache",
        ] {
            assert!(status_line(line).is_some(), "nothing to say about {line:?}");
        }
    }

    /// "Copy this and run it" has to be a line that works where it is pasted.
    #[test]
    fn the_install_command_matches_the_distribution() {
        let typed = |os: &str| {
            package_command(os, "flatpak-builder").map(|command| command.as_typed())
        };

        assert_eq!(
            typed("NAME=\"Fedora Linux\"\nID=fedora\n").as_deref(),
            Some("sudo dnf install flatpak-builder")
        );
        assert_eq!(
            typed("ID=ubuntu\nID_LIKE=debian\n").as_deref(),
            Some("sudo apt install flatpak-builder")
        );
        assert_eq!(
            typed("ID=arch\n").as_deref(),
            Some("sudo pacman -S flatpak-builder")
        );
        // A derivative names its parent and nothing else we know.
        assert_eq!(
            typed("ID=nobara\nID_LIKE=fedora\n").as_deref(),
            Some("sudo dnf install flatpak-builder")
        );
        assert_eq!(
            typed("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n").as_deref(),
            Some("sudo apt install flatpak-builder")
        );
    }

    /// Better no command than one that fails: an unknown distribution is told
    /// the package's name instead of being handed somebody else's package
    /// manager.
    #[test]
    fn an_unknown_distribution_gets_the_package_name_instead() {
        assert!(package_command("ID=something-nobody-has\n", "flatpak").is_none());
        assert!(package_command("", "flatpak").is_none());

        let (extra, fix) = install_fix_for("", "flatpak-builder");
        assert!(extra.contains("flatpak-builder"), "{extra}");
        assert!(fix.is_none());

        let (extra, fix) = install_fix_for("ID=fedora\n", "flatpak-builder");
        assert!(extra.is_empty(), "{extra}");
        assert!(fix.is_some());
    }
}
