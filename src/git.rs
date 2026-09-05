//! Starting from an address instead of a folder.
//!
//! Pasting a GitHub link is the way most people would like to begin, and it very
//! nearly works on its own: the code has to be fetched once so the app can look
//! at it — to see that it is a Rust project, to read its Cargo.lock, to find out
//! what the program is called — and after that the manifest points at the
//! repository rather than at the copy, so anyone can build it from the address
//! alone.
//!
//! The pinned commit is the important part. A manifest naming a branch builds
//! something different every day; one naming a commit builds the same thing in a
//! year's time, which is the whole promise of Flatpak.

use std::path::Path;

use crate::manifest::{Source, SourceEntry, SourceKind};
use crate::project::Project;

/// What is wrong with what was pasted, in the user's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlProblem {
    Empty,
    NotAnAddress,
    /// A web page rather than a repository — a file view, an issue, a release.
    PageNotRepository(String),
}

impl UrlProblem {
    pub fn message(&self) -> String {
        match self {
            UrlProblem::Empty => "Paste the address of the repository.".into(),
            UrlProblem::NotAnAddress => {
                "That doesn't look like a repository address. It should start with \
                 https:// or git@, like https://github.com/someone/project."
                    .into()
            }
            UrlProblem::PageNotRepository(cleaned) => format!(
                "That's a link to a page inside the project rather than to the project \
                 itself. The address to use is {cleaned}"
            ),
        }
    }
}

/// Tidy up what was pasted, or say what is wrong with it.
///
/// People paste what their browser shows them: a `tree/main/src` link, a
/// releases page, an address with a trailing slash. All of those name the same
/// repository, so they are trimmed back to it rather than rejected.
pub fn normalise_url(input: &str) -> Result<String, UrlProblem> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UrlProblem::Empty);
    }

    // ssh form: git@github.com:someone/project.git
    if let Some(rest) = text.strip_prefix("git@") {
        return if rest.contains(':') && rest.contains('/') {
            Ok(text.to_string())
        } else {
            Err(UrlProblem::NotAnAddress)
        };
    }

    if !text.starts_with("https://") && !text.starts_with("http://") {
        return Err(UrlProblem::NotAnAddress);
    }

    let without_scheme = text
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let mut parts: Vec<&str> = without_scheme
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();

    // host + owner + repository is the shortest thing that can be cloned.
    if parts.len() < 3 {
        return Err(UrlProblem::NotAnAddress);
    }

    // Anything past the repository is a page: tree/…, blob/…, issues, releases.
    let trimmed = parts.len() > 3;
    parts.truncate(3);
    let repo = parts[2].trim_end_matches(".git");
    let cleaned = format!("https://{}/{}/{repo}", parts[0], parts[1]);

    if trimmed {
        return Err(UrlProblem::PageNotRepository(cleaned));
    }
    Ok(cleaned)
}

/// The repository's own name, which is the obvious name for the folder and a
/// decent first guess at the name of the app.
pub fn repo_name(url: &str) -> String {
    let name = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("project")
        .trim_end_matches(".git");

    if name.is_empty() {
        "project".to_string()
    } else {
        name.to_string()
    }
}

/// Why a destination can't be used. Cloning into a folder that already has
/// things in it is the one mistake that loses somebody's work, so it is refused
/// rather than merged into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestinationProblem {
    NotEmpty,
    NotAFolder,
    Unwritable,
}

impl DestinationProblem {
    pub fn message(&self) -> String {
        match self {
            DestinationProblem::NotEmpty => {
                "There is already something in that folder. Choose an empty one, or a \
                 name that doesn't exist yet — the code is copied into it as it is."
                    .into()
            }
            DestinationProblem::NotAFolder => {
                "That is a file, not a folder. The code needs a folder of its own.".into()
            }
            DestinationProblem::Unwritable => {
                "That folder can't be written to. Choose somewhere inside your home \
                 folder."
                    .into()
            }
        }
    }
}

pub fn check_destination(path: &Path) -> Result<(), DestinationProblem> {
    if path.is_file() {
        return Err(DestinationProblem::NotAFolder);
    }
    if path.is_dir() {
        let empty = std::fs::read_dir(path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        return if empty {
            Ok(())
        } else {
            Err(DestinationProblem::NotEmpty)
        };
    }

    // It doesn't exist yet, which is fine as long as its parent does.
    match path.parent() {
        Some(parent) if parent.is_dir() => Ok(()),
        Some(parent) if parent.as_os_str().is_empty() => Ok(()),
        _ => Err(DestinationProblem::Unwritable),
    }
}

/// `git clone`, shallow: the app only needs to look at the code, and a full
/// history of a large project is a long wait for nothing.
pub fn clone_command(url: &str, version: Option<&str>, destination: &Path) -> Vec<String> {
    let mut argv = vec![
        "git".to_string(),
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--progress".to_string(),
    ];
    if let Some(version) = version.map(str::trim).filter(|v| !v.is_empty()) {
        argv.push("--branch".to_string());
        argv.push(version.to_string());
    }
    argv.push(url.to_string());
    argv.push(destination.display().to_string());
    argv
}

/// What was actually checked out. The manifest pins this, not the branch.
pub fn head_command(destination: &Path) -> Vec<String> {
    vec![
        "git".to_string(),
        "-C".to_string(),
        destination.display().to_string(),
        "rev-parse".to_string(),
        "HEAD".to_string(),
    ]
}

pub fn parse_head(output: &str) -> Option<String> {
    let commit = output.trim();
    (commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| commit.to_string())
}

/// Point the project's module at the repository rather than at the copy on this
/// computer, so the manifest builds anywhere.
pub fn use_repository(project: &mut Project, url: &str, commit: &str, tag: Option<&str>) {
    let name = project
        .manifest
        .main_module()
        .map(|module| module.name.clone())
        .unwrap_or_else(|| repo_name(url));

    let source = Source {
        kind: SourceKind::Git,
        url: Some(url.to_string()),
        commit: Some(commit.to_string()),
        tag: tag
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .map(str::to_string),
        ..Source::default()
    };

    let module = project.manifest.ensure_main_module(&name);
    // Replace the folder source that detection added: the same code, named the
    // way everyone else can reach it.
    module
        .sources
        .retain(|entry| !matches!(entry.as_source(), Some(source) if source.kind == SourceKind::Dir));
    module.sources.insert(0, SourceEntry::Source(source));
}

/// A line of git's own progress, turned into something worth showing.
pub fn status_line(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if lower.contains("counting objects") || lower.contains("enumerating objects") {
        Some("Asking the server what's there".to_string())
    } else if lower.contains("receiving objects") {
        Some("Copying the code across".to_string())
    } else if lower.contains("resolving deltas") {
        Some("Putting the files together".to_string())
    } else if lower.contains("updating files") || lower.contains("checking out") {
        Some("Writing the files out".to_string())
    } else {
        None
    }
}

/// What git said, when it failed, in the user's terms.
pub fn diagnose(log: &str) -> String {
    let lower = log.to_lowercase();

    // Order matters: "git: command not found" contains "not found", and
    // reporting a missing git as a missing repository would send someone off
    // checking an address that was fine all along.
    if lower.contains("command not found") || lower.contains("executable file not found") {
        "Git isn't installed on this computer. Your distribution packages it as “git”."
            .into()
    } else if lower.contains("could not resolve host") || lower.contains("network is unreachable") {
        "This computer couldn't reach the server. Check the connection and the address."
            .into()
    } else if lower.contains("remote branch") {
        "There is no tag or branch by that name in this repository. Leave it empty to \
         take whatever is current."
            .into()
    } else if lower.contains("authentication failed") || lower.contains("permission denied") {
        "The server wants a username and password. This app doesn't ask for those — \
         clone it yourself and start from the folder instead."
            .into()
    } else if lower.contains("repository not found")
        || lower.contains("could not read from remote repository")
        || (lower.contains("repository") && lower.contains("not found"))
    {
        "The server says there's no repository at that address. If it's a private one, \
         this app can't reach it — clone it yourself and start from the folder instead."
            .into()
    } else if lower.contains("already exists and is not an empty directory") {
        "There is already something in that folder. Choose an empty one.".into()
    } else {
        "The code couldn't be fetched. The last thing git said is below.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;

    #[test]
    fn ordinary_addresses_are_taken_as_they_are() {
        assert_eq!(
            normalise_url("https://github.com/someone/project").unwrap(),
            "https://github.com/someone/project"
        );
        assert_eq!(
            normalise_url("  https://github.com/someone/project.git/ ").unwrap(),
            "https://github.com/someone/project"
        );
        assert_eq!(
            normalise_url("https://codeberg.org/someone/project").unwrap(),
            "https://codeberg.org/someone/project"
        );
        assert_eq!(
            normalise_url("git@github.com:someone/project.git").unwrap(),
            "git@github.com:someone/project.git"
        );
    }

    #[test]
    fn a_link_to_a_page_says_what_the_address_should_be() {
        let err = normalise_url("https://github.com/someone/project/tree/main/src").unwrap_err();
        assert_eq!(
            err,
            UrlProblem::PageNotRepository("https://github.com/someone/project".into())
        );
        assert!(err.message().contains("https://github.com/someone/project"));

        assert!(matches!(
            normalise_url("https://github.com/someone/project/releases/tag/v1.0"),
            Err(UrlProblem::PageNotRepository(_))
        ));
    }

    #[test]
    fn what_isnt_an_address_is_refused_with_a_reason() {
        assert_eq!(normalise_url("   ").unwrap_err(), UrlProblem::Empty);
        assert_eq!(
            normalise_url("github.com/someone/project").unwrap_err(),
            UrlProblem::NotAnAddress
        );
        assert_eq!(
            normalise_url("https://github.com").unwrap_err(),
            UrlProblem::NotAnAddress
        );
        assert!(normalise_url("not an address at all")
            .unwrap_err()
            .message()
            .contains("https://"));
    }

    #[test]
    fn the_folder_is_named_after_the_repository() {
        assert_eq!(repo_name("https://github.com/someone/my-project"), "my-project");
        assert_eq!(repo_name("https://github.com/someone/my-project.git"), "my-project");
        assert_eq!(repo_name("git@github.com:someone/my-project.git"), "my-project");
    }

    #[test]
    fn cloning_is_shallow_and_can_ask_for_a_tag() {
        let plain = clone_command(
            "https://github.com/someone/project",
            None,
            Path::new("/home/me/project"),
        );
        assert_eq!(
            plain.join(" "),
            "git clone --depth 1 --progress https://github.com/someone/project /home/me/project"
        );

        let tagged = clone_command(
            "https://github.com/someone/project",
            Some(" v1.2.0 "),
            Path::new("/home/me/project"),
        );
        assert!(tagged.windows(2).any(|pair| pair == ["--branch", "v1.2.0"]));

        // An empty version box is the same as not asking for one.
        assert_eq!(
            clone_command("u", Some("  "), Path::new("/d")),
            clone_command("u", None, Path::new("/d"))
        );
    }

    #[test]
    fn the_commit_is_read_back_and_checked() {
        assert_eq!(
            parse_head("0a1b2c3d4e5f60718293a4b5c6d7e8f901234567\n").as_deref(),
            Some("0a1b2c3d4e5f60718293a4b5c6d7e8f901234567")
        );
        assert!(parse_head("fatal: not a git repository").is_none());
        assert!(parse_head("").is_none());
        assert_eq!(
            head_command(Path::new("/home/me/project")).join(" "),
            "git -C /home/me/project rev-parse HEAD"
        );
    }

    #[test]
    fn an_empty_or_missing_folder_is_fine_and_a_full_one_is_not() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_destination(&dir.path().join("new-project")).is_ok());
        assert!(check_destination(dir.path()).is_ok());

        std::fs::write(dir.path().join("something"), b"x").unwrap();
        assert_eq!(
            check_destination(dir.path()).unwrap_err(),
            DestinationProblem::NotEmpty
        );
        assert_eq!(
            check_destination(&dir.path().join("something")).unwrap_err(),
            DestinationProblem::NotAFolder
        );
        assert!(check_destination(&dir.path().join("no/such/parent")).is_err());
    }

    #[test]
    fn the_manifest_ends_up_pointing_at_the_repository_not_the_copy() {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.Sample\nmodules:\n  - name: sample\n    sources:\n\
             \x20     - type: dir\n        path: .\n",
        )
        .unwrap();
        let mut project = Project::from_import(&import, None);

        use_repository(
            &mut project,
            "https://github.com/someone/project",
            "0a1b2c3d4e5f60718293a4b5c6d7e8f901234567",
            Some("v1.2.0"),
        );

        let module = project.manifest.main_module().unwrap();
        assert_eq!(module.sources.len(), 1, "the local folder source is replaced");

        let source = module.sources[0].as_source().unwrap();
        assert_eq!(source.kind, SourceKind::Git);
        assert_eq!(source.url.as_deref(), Some("https://github.com/someone/project"));
        // Pinned to a commit: the same manifest builds the same thing next year.
        assert_eq!(
            source.commit.as_deref(),
            Some("0a1b2c3d4e5f60718293a4b5c6d7e8f901234567")
        );
        assert_eq!(source.tag.as_deref(), Some("v1.2.0"));
    }

    #[test]
    fn other_sources_are_left_alone() {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.Sample\nmodules:\n  - name: sample\n    sources:\n\
             \x20     - type: dir\n        path: .\n      - ../generated-sources.json\n",
        )
        .unwrap();
        let mut project = Project::from_import(&import, None);
        use_repository(&mut project, "https://x/y", "abcdef1", None);

        let module = project.manifest.main_module().unwrap();
        assert_eq!(module.sources.len(), 2);
        assert!(module.sources.iter().any(
            |entry| matches!(entry, SourceEntry::Include(path) if path.ends_with("generated-sources.json"))
        ));
    }

    #[test]
    fn gits_progress_becomes_something_readable() {
        assert!(status_line("Receiving objects:  42% (100/238)")
            .unwrap()
            .contains("Copying"));
        assert!(status_line("Resolving deltas: 100% (50/50)").is_some());
        assert!(status_line("some other line").is_none());
    }

    #[test]
    fn failures_are_explained_rather_than_quoted() {
        assert!(diagnose("fatal: repository 'https://x/y' not found")
            .contains("no repository at that address"));
        assert!(diagnose("fatal: could not resolve host: github.com").contains("couldn't reach"));
        assert!(diagnose("Authentication failed for 'https://x/y'").contains("username and password"));
        assert!(diagnose("git: command not found").contains("Git isn't installed"));
        assert!(diagnose("fatal: Remote branch v9 not found in upstream origin")
            .contains("no tag or branch by that name"));
        assert!(diagnose("something unexpected").contains("last thing git said"));
    }
}
