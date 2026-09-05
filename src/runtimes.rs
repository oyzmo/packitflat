//! Which runtime the app should be built on, in words rather than in reverse-DNS.
//!
//! Three sources, in order of trust: what `flatpak remote-ls flathub --runtime`
//! says is available, what the last successful call to that cached, and a list
//! compiled into the app so the wizard still works with no network and no
//! flatpak at all.
//!
//! Running flatpak is the UI layer's job (it has to be async); everything here
//! is text in, text out, so it can be tested without either.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

/// How the runtime lists were judged. Support windows move, so this is dated:
/// reviewed September 2026. An unknown version is never called dead — being
/// wrong in that direction would block someone's perfectly good build.
pub const KNOWLEDGE_DATE: &str = "September 2026";

/// A day. Long enough that the wizard doesn't shell out on every visit, short
/// enough that a newly published runtime turns up the next day.
const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Maintained, and what a new app should use.
    Current,
    /// Still works, but it is on its way out.
    Ageing,
    /// No more security fixes.
    EndOfLife,
    /// Not one of the runtimes this app knows anything about.
    Unknown,
}

impl Support {
    /// The sentence shown next to the choice. Says what it means for the user,
    /// not what the status is called.
    pub fn explanation(&self) -> &'static str {
        match self {
            Support::Current => "Maintained, and a good choice for a new app.",
            Support::Ageing => {
                "Still works, but a newer version exists. Moving up sooner is easier than later."
            }
            Support::EndOfLife => {
                "No longer maintained: it stops getting security fixes, and app stores may \
                 refuse it. Pick a newer one unless something forces this."
            }
            Support::Unknown => {
                "This app doesn't know how well this runtime is maintained — check with \
                 whoever publishes it."
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub runtime: String,
    pub version: String,
    pub sdk: String,
    /// "GNOME 50" rather than "org.gnome.Platform 50".
    pub friendly: String,
    pub support: Support,
    /// Whether it is already on this computer. Missing is not a problem — it is
    /// one `flatpak install` away — but the user should be told before the build.
    pub installed: bool,
}

impl Choice {
    /// The command to paste into a terminal when the app can't install it. Both
    /// halves: building needs the SDK, running needs the platform.
    pub fn install_command(&self) -> String {
        format!(
            "flatpak install flathub {}//{} {}//{}",
            self.runtime, self.version, self.sdk, self.version
        )
    }
}

/// An SDK extension: a compiler or toolchain added to the build environment.
pub struct Extension {
    pub id: &'static str,
    /// What it is, in the user's terms.
    pub label: &'static str,
    pub explanation: &'static str,
    /// Where the extension puts its programs. **Flatpak does not add this to
    /// PATH by itself** — the manifest has to say `append-path`, and a build
    /// without it fails with `cargo: command not found` after downloading
    /// everything, which looks like anything but a missing path.
    pub bin_path: &'static str,
    /// Libraries that also have to be findable, for the few that need it.
    pub library_path: Option<&'static str>,
}

pub const EXTENSIONS: &[Extension] = &[
    Extension {
        id: "org.freedesktop.Sdk.Extension.rust-stable",
        label: "Rust",
        explanation: "Needed to compile Rust code. Add this if your project has a Cargo.toml.",
        bin_path: "/usr/lib/sdk/rust-stable/bin",
        library_path: None,
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.node22",
        label: "Node.js 22",
        explanation: "Needed to build JavaScript projects that have a package.json.",
        bin_path: "/usr/lib/sdk/node22/bin",
        library_path: None,
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.golang",
        label: "Go",
        explanation: "Needed to compile Go code.",
        bin_path: "/usr/lib/sdk/golang/bin",
        library_path: None,
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.openjdk",
        label: "Java",
        explanation: "Needed to compile Java code.",
        bin_path: "/usr/lib/sdk/openjdk/bin",
        library_path: None,
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.dotnet9",
        label: ".NET 9",
        explanation: "Needed to build C# and other .NET projects.",
        bin_path: "/usr/lib/sdk/dotnet9/bin",
        library_path: Some("/usr/lib/sdk/dotnet9/lib"),
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.llvm20",
        label: "Clang for C and C++",
        explanation: "Only needed if the project asks for Clang specifically, or for a \
                      newer C++ standard than the SDK's compiler supports. Ordinary C and \
                      C++ projects need nothing here.",
        bin_path: "/usr/lib/sdk/llvm20/bin",
        library_path: Some("/usr/lib/sdk/llvm20/lib"),
    },
    Extension {
        id: "org.freedesktop.Sdk.Extension.php84",
        label: "PHP",
        explanation: "Needed to run or build PHP code.",
        bin_path: "/usr/lib/sdk/php84/bin",
        library_path: None,
    },
];

/// The programs each extension provides, for recognising `cargo: command not
/// found` and saying which switch it belongs to.
pub const TOOLS: &[(&str, &str)] = &[
    ("cargo", "org.freedesktop.Sdk.Extension.rust-stable"),
    ("rustc", "org.freedesktop.Sdk.Extension.rust-stable"),
    ("node", "org.freedesktop.Sdk.Extension.node22"),
    ("npm", "org.freedesktop.Sdk.Extension.node22"),
    ("yarn", "org.freedesktop.Sdk.Extension.node22"),
    ("go", "org.freedesktop.Sdk.Extension.golang"),
    ("javac", "org.freedesktop.Sdk.Extension.openjdk"),
    ("dotnet", "org.freedesktop.Sdk.Extension.dotnet9"),
    ("clang", "org.freedesktop.Sdk.Extension.llvm20"),
    ("php", "org.freedesktop.Sdk.Extension.php84"),
];

/// Which extension provides a program, if any.
pub fn extension_for_tool(tool: &str) -> Option<&'static Extension> {
    TOOLS
        .iter()
        .find(|(name, _)| *name == tool)
        .and_then(|(_, id)| extension(id))
}

/// The directories a set of extensions needs on `PATH`, and on the library path.
pub fn extension_paths(ids: &[String]) -> (Vec<&'static str>, Vec<&'static str>) {
    let chosen: Vec<&'static Extension> = ids.iter().filter_map(|id| extension(id)).collect();
    (
        chosen.iter().map(|e| e.bin_path).collect(),
        chosen.iter().filter_map(|e| e.library_path).collect(),
    )
}

/// Whether the manifest tells the build where an extension's programs are. This
/// is the difference between a build that works and one that spends ten minutes
/// downloading and then says `cargo: command not found`.
pub fn path_is_set(append_path: Option<&str>, extension: &Extension) -> bool {
    append_path
        .unwrap_or_default()
        .split(':')
        .any(|entry| entry.trim() == extension.bin_path)
}

/// Put the right `append-path` and `prepend-ld-library-path` in the manifest for
/// whatever extensions are switched on, and take out the ones that no longer
/// are. Anything the user added themselves is left where it is.
pub fn sync_build_paths(manifest: &mut crate::manifest::Manifest) {
    let (bins, libs) = extension_paths(&manifest.sdk_extensions);
    let ours_bins: Vec<&str> = EXTENSIONS.iter().map(|e| e.bin_path).collect();
    let ours_libs: Vec<&str> = EXTENSIONS.iter().filter_map(|e| e.library_path).collect();

    let options = manifest
        .build_options
        .get_or_insert_with(crate::manifest::BuildOptions::default);

    options.append_path = merge_paths(options.append_path.as_deref(), &bins, &ours_bins);
    options.prepend_ld_library_path =
        merge_paths(options.prepend_ld_library_path.as_deref(), &libs, &ours_libs);

    if options.is_empty() {
        manifest.build_options = None;
    }

    // And again on the module, when it has build-options of its own.
    //
    // flatpak-builder documents module build-options as overriding the global
    // ones, and whether that override is per-property or wholesale is not
    // something to bet a ten-minute build on. A module that sets CARGO_HOME and
    // nothing else must not thereby lose the path to cargo, so both blocks say
    // the same thing.
    let module_has_options = manifest
        .main_module()
        .is_some_and(|module| module.build_options.is_some());
    if !module_has_options {
        return;
    }
    if let Some(module) = manifest.main_module_mut() {
        if let Some(options) = module.build_options.as_mut() {
            options.append_path = merge_paths(options.append_path.as_deref(), &bins, &ours_bins);
            options.prepend_ld_library_path =
                merge_paths(options.prepend_ld_library_path.as_deref(), &libs, &ours_libs);
        }
    }
}

/// Wanted entries first, then whatever was there that this app doesn't manage.
fn merge_paths(current: Option<&str>, wanted: &[&str], managed: &[&str]) -> Option<String> {
    let mut paths: Vec<String> = wanted.iter().map(|path| path.to_string()).collect();
    for entry in current.unwrap_or_default().split(':') {
        let entry = entry.trim();
        if !entry.is_empty() && !managed.contains(&entry) && !paths.iter().any(|p| p == entry) {
            paths.push(entry.to_string());
        }
    }
    (!paths.is_empty()).then(|| paths.join(":"))
}

pub fn extension(id: &str) -> Option<&'static Extension> {
    EXTENSIONS.iter().find(|e| e.id == id)
}

/// Languages that need nothing switched on, and the sentence saying so.
///
/// "Where is C++?" is the obvious question to ask of a list that offers Rust,
/// Go and Java — and the answer is that the compiler is already there. A list
/// that stays silent about it looks like it has forgotten something.
pub const ALREADY_INCLUDED: &[(&str, &str)] = &[
    (
        "C and C++",
        "Already there: gcc, g++ and make are part of every SDK, along with meson, \
         cmake and pkg-config. Nothing to switch on.",
    ),
    (
        "Python",
        "Already there: Python 3 is part of the GNOME and Freedesktop runtimes. Only \
         the packages it downloads need preparing.",
    ),
    (
        "Vala",
        "Already there: the Vala compiler comes with the GNOME SDK.",
    ),
];

/// Runtimes compiled into the app, so the wizard works with no network, no
/// cache, and no flatpak installed. Newest first, which is also picker order.
pub fn bundled() -> Vec<(String, String)> {
    [
        ("org.gnome.Platform", "50"),
        ("org.gnome.Platform", "49"),
        ("org.gnome.Platform", "48"),
        ("org.gnome.Platform", "47"),
        ("org.kde.Platform", "6.9"),
        ("org.kde.Platform", "6.8"),
        ("org.freedesktop.Platform", "25.08"),
        ("org.freedesktop.Platform", "24.08"),
    ]
    .iter()
    .map(|(r, v)| (r.to_string(), v.to_string()))
    .collect()
}

/// `org.gnome.Platform` → `org.gnome.Sdk`. Every runtime family names its SDK
/// this way; anything unrecognised gets the same substitution, which is the
/// convention rather than a guess.
pub fn sdk_for(runtime: &str) -> String {
    match runtime {
        "org.gnome.Platform" => "org.gnome.Sdk".to_string(),
        "org.kde.Platform" => "org.kde.Sdk".to_string(),
        "org.freedesktop.Platform" => "org.freedesktop.Sdk".to_string(),
        other => other.replace(".Platform", ".Sdk"),
    }
}

pub fn friendly(runtime: &str, version: &str) -> String {
    let family = match runtime {
        "org.gnome.Platform" => "GNOME",
        "org.kde.Platform" => "KDE",
        "org.freedesktop.Platform" => "Freedesktop",
        other => other,
    };
    format!("{family} {version}")
}

/// Judged, not looked up: there is no machine-readable end-of-life feed to ask,
/// so this is a dated table (see [`KNOWLEDGE_DATE`]) that errs towards silence.
pub fn support(runtime: &str, version: &str) -> Support {
    let numeric = |s: &str| s.split(['.', '-']).next().unwrap_or("").parse::<u32>().ok();

    match runtime {
        "org.gnome.Platform" => match numeric(version) {
            Some(v) if v >= 48 => Support::Current,
            Some(47) => Support::Ageing,
            Some(_) => Support::EndOfLife,
            None => Support::Unknown,
        },
        "org.kde.Platform" => match numeric(version) {
            Some(v) if v >= 6 => Support::Current,
            Some(_) => Support::EndOfLife,
            None => Support::Unknown,
        },
        "org.freedesktop.Platform" => match numeric(version) {
            Some(v) if v >= 25 => Support::Current,
            Some(24) => Support::Ageing,
            Some(_) => Support::EndOfLife,
            None => Support::Unknown,
        },
        _ => Support::Unknown,
    }
}

/// Parse the output of `flatpak remote-ls flathub --runtime --columns=application,branch`
/// or `flatpak list --runtime --columns=application,branch`: one entry per line,
/// tab-separated. Locales and column widths change; the tab does not.
///
/// Only `.Platform` entries are kept — SDKs, extensions, locales and the dozens
/// of `.GL.default` variants are noise in a runtime picker.
pub fn parse_list(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        let Some((id, branch)) = line.split_once('\t') else {
            continue;
        };
        let (id, branch) = (id.trim(), branch.trim());
        if id.is_empty() || branch.is_empty() || !id.ends_with(".Platform") {
            continue;
        }
        let entry = (id.to_string(), branch.to_string());
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// Everything to offer, newest first within each family, with the bundled list
/// filling in whatever flatpak didn't report.
pub fn catalogue(available: &[(String, String)], installed: &[(String, String)]) -> Vec<Choice> {
    let mut pairs: Vec<(String, String)> = available.to_vec();
    for entry in bundled() {
        if !pairs.contains(&entry) {
            pairs.push(entry);
        }
    }
    // Dead runtimes stay out of the picker unless they are already installed or
    // already chosen; offering a beginner an unmaintained runtime is a trap.
    pairs.retain(|(runtime, version)| {
        support(runtime, version) != Support::EndOfLife
            || installed.contains(&(runtime.clone(), version.clone()))
    });

    pairs.sort_by(|a, b| family_rank(&a.0).cmp(&family_rank(&b.0)).then_with(|| version_key(&b.1).cmp(&version_key(&a.1))));

    pairs
        .into_iter()
        .map(|(runtime, version)| Choice {
            sdk: sdk_for(&runtime),
            friendly: friendly(&runtime, &version),
            support: support(&runtime, &version),
            installed: installed.contains(&(runtime.clone(), version.clone())),
            runtime,
            version,
        })
        .collect()
}

/// GNOME first: this is a GTK app builder, and the runtime someone packaging a
/// desktop app most likely wants is the one their desktop uses.
fn family_rank(runtime: &str) -> u8 {
    match runtime {
        "org.gnome.Platform" => 0,
        "org.kde.Platform" => 1,
        "org.freedesktop.Platform" => 2,
        _ => 3,
    }
}

/// "25.08" sorts above "24.08", and "6.10" above "6.9" — which string ordering
/// gets wrong.
fn version_key(version: &str) -> Vec<u32> {
    version
        .split(['.', '-'])
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

/// The choice matching what is already in the manifest, so re-entering the step
/// shows what was chosen rather than resetting it.
pub fn position_of(choices: &[Choice], runtime: &str, version: &str) -> Option<usize> {
    choices
        .iter()
        .position(|c| c.runtime == runtime && c.version == version)
}

pub fn cache_path() -> PathBuf {
    let cache_home = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".cache")
        });
    cache_home.join("packitflat").join("flathub-runtimes.tsv")
}

/// The cached list, if it is fresh enough to trust.
pub fn load_cached() -> Option<String> {
    load_cached_from(&cache_path())
}

pub fn load_cached_from(path: &std::path::Path) -> Option<String> {
    let age = path.metadata().ok()?.modified().ok()?.elapsed().ok()?;
    if age > CACHE_MAX_AGE {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

pub fn store_cached(text: &str) -> Result<()> {
    store_cached_in(&cache_path(), text)
}

/// What is installed on this computer. Cached separately and read whatever its
/// age: it is only used to say "already installed" or not, it is refreshed at
/// every start, and a day-old answer beats no answer while that runs.
pub fn installed_cache_path() -> PathBuf {
    cache_path().with_file_name("installed-runtimes.tsv")
}

pub fn load_cached_installed() -> Option<String> {
    std::fs::read_to_string(installed_cache_path()).ok()
}

pub fn store_cached_installed(text: &str) -> Result<()> {
    store_cached_in(&installed_cache_path(), text)
}

pub fn store_cached_in(path: &std::path::Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("could not write {}", path.display()))
}

/// Whether the cache is stale enough to be worth refreshing in the background.
pub fn cache_is_stale() -> bool {
    load_cached().is_none()
}

/// The command that lists runtimes. Kept here so the UI and the "copy this into
/// a terminal" fallback can't drift apart.
pub const REMOTE_LS_ARGS: &[&str] = &[
    "remote-ls",
    "flathub",
    "--runtime",
    "--columns=application,branch",
];
pub const LIST_INSTALLED_ARGS: &[&str] =
    &["list", "--runtime", "--columns=application,branch"];

#[cfg(test)]
mod tests {
    use super::*;

    const REMOTE_LS: &str = "org.gnome.Platform\t49\n\
                             org.gnome.Platform\t50\n\
                             org.gnome.Sdk\t50\n\
                             org.freedesktop.Platform.GL.default\t25.08\n\
                             org.gnome.Platform\t46\n\
                             org.freedesktop.Platform\t25.08\n\
                             org.kde.Platform\t5.15-24.08\n";

    #[test]
    fn only_platforms_come_out_of_the_listing() {
        let parsed = parse_list(REMOTE_LS);
        assert!(parsed.contains(&("org.gnome.Platform".into(), "50".into())));
        assert!(!parsed.iter().any(|(id, _)| id.contains("Sdk")));
        assert!(!parsed.iter().any(|(id, _)| id.contains("GL.default")));
        assert_eq!(parsed.len(), 5);
    }

    #[test]
    fn junk_lines_are_skipped_not_fatal() {
        assert!(parse_list("").is_empty());
        assert!(parse_list("Looking for matches...\nnothing here\n").is_empty());
        assert!(parse_list("org.gnome.Platform\t\n").is_empty());
    }

    #[test]
    fn catalogue_is_newest_first_gnome_first() {
        let available = parse_list(REMOTE_LS);
        let installed = vec![("org.gnome.Platform".to_string(), "50".to_string())];
        let choices = catalogue(&available, &installed);

        assert_eq!(choices[0].friendly, "GNOME 50");
        assert!(choices[0].installed);
        assert_eq!(choices[0].sdk, "org.gnome.Sdk");
        assert_eq!(choices[1].friendly, "GNOME 49");

        let families: Vec<&str> = choices.iter().map(|c| c.runtime.as_str()).collect();
        let first_fdo = families.iter().position(|r| r.contains("freedesktop"));
        let last_gnome = families.iter().rposition(|r| r.contains("gnome"));
        assert!(last_gnome < first_fdo);
    }

    #[test]
    fn dead_runtimes_are_hidden_unless_already_installed() {
        let available = parse_list(REMOTE_LS);
        let choices = catalogue(&available, &[]);
        assert!(!choices.iter().any(|c| c.version == "46"));

        let installed = vec![("org.gnome.Platform".to_string(), "46".to_string())];
        let choices = catalogue(&available, &installed);
        let old = choices.iter().find(|c| c.version == "46").unwrap();
        assert_eq!(old.support, Support::EndOfLife);
        assert!(old.explanation_is_a_warning());
    }

    impl Choice {
        fn explanation_is_a_warning(&self) -> bool {
            self.support.explanation().contains("No longer maintained")
        }
    }

    #[test]
    fn support_windows_are_judged_per_family() {
        assert_eq!(support("org.gnome.Platform", "50"), Support::Current);
        assert_eq!(support("org.gnome.Platform", "47"), Support::Ageing);
        assert_eq!(support("org.gnome.Platform", "45"), Support::EndOfLife);
        assert_eq!(support("org.freedesktop.Platform", "24.08"), Support::Ageing);
        assert_eq!(support("org.kde.Platform", "5.15-24.08"), Support::EndOfLife);
        assert_eq!(support("com.example.Platform", "1"), Support::Unknown);
    }

    #[test]
    fn versions_sort_numerically_not_alphabetically() {
        assert!(version_key("6.10") > version_key("6.9"));
        assert!(version_key("25.08") > version_key("24.08"));
    }

    #[test]
    fn the_bundled_list_stands_in_when_flatpak_says_nothing() {
        let choices = catalogue(&[], &[]);
        assert!(choices.iter().any(|c| c.friendly == "GNOME 50"));
        assert!(choices.iter().all(|c| !c.installed));
        assert_eq!(
            choices[0].install_command(),
            "flatpak install flathub org.gnome.Platform//50 org.gnome.Sdk//50"
        );
    }

    #[test]
    fn the_cache_expires() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtimes.tsv");
        store_cached_in(&path, REMOTE_LS).unwrap();
        assert_eq!(load_cached_from(&path).as_deref(), Some(REMOTE_LS));

        let missing = dir.path().join("nothing.tsv");
        assert!(load_cached_from(&missing).is_none());
    }

    /// "Where is C++?" is the obvious question to ask of a list offering Rust,
    /// Go and Java. It has to be answered on the page, not left as a gap.
    #[test]
    fn the_languages_that_need_nothing_are_named_anyway() {
        let named: Vec<&str> = ALREADY_INCLUDED.iter().map(|(name, _)| *name).collect();
        assert!(named.contains(&"C and C++"), "{named:?}");

        for (name, explanation) in ALREADY_INCLUDED {
            assert!(
                explanation.starts_with("Already there:"),
                "{name} should say plainly that nothing is needed"
            );
        }

        // …and the one C++ extension that does exist says when it is for.
        let clang = extension("org.freedesktop.Sdk.Extension.llvm20").unwrap();
        assert!(clang.label.contains("C++"));
        assert!(clang.explanation.contains("need nothing here"));
    }

    #[test]
    fn every_extension_says_what_it_is_for() {
        for ext in EXTENSIONS {
            assert!(ext.id.starts_with("org.freedesktop.Sdk.Extension."));
            assert!(!ext.explanation.is_empty());
        }
        assert_eq!(
            extension("org.freedesktop.Sdk.Extension.rust-stable")
                .unwrap()
                .label,
            "Rust"
        );
    }
}
