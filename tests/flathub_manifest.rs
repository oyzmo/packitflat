//! The Flathub copy of the manifest has to say the same thing as the one that
//! is actually built here.
//!
//! There are two manifests on purpose: `flatpak/no.oyzmo.PackItFlat.yml` builds
//! the working tree with a `dir` source, and Flathub does not accept that — it
//! builds only from a published address. So the copy under `flatpak/flathub/`
//! differs in its sources and in nothing else. Nothing builds that copy here,
//! which is exactly why it would drift: a permission added to the real manifest
//! and forgotten in the submission is a round trip with a reviewer, and a build
//! command forgotten there is an app that installs without its icon.

use std::path::Path;

use serde_yaml_ng::Value;

fn read(path: &str) -> Value {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|err| panic!("{path}: {err}"));
    serde_yaml_ng::from_str(&text).unwrap_or_else(|err| panic!("{path} doesn't parse: {err}"))
}

fn local() -> Value {
    read("flatpak/no.oyzmo.PackItFlat.yml")
}

fn flathub() -> Value {
    read("flatpak/flathub/no.oyzmo.PackItFlat.yml")
}

#[test]
fn the_two_manifests_agree_on_everything_but_where_the_code_comes_from() {
    let (local, flathub) = (local(), flathub());

    for key in [
        "app-id",
        "runtime",
        "runtime-version",
        "sdk",
        "sdk-extensions",
        "command",
        "finish-args",
        "build-options",
    ] {
        assert_eq!(
            local.get(key),
            flathub.get(key),
            "the Flathub manifest disagrees about `{key}`"
        );
    }

    let module = |manifest: &Value| manifest["modules"][0].clone();
    assert_eq!(
        module(&local)["build-commands"],
        module(&flathub)["build-commands"],
        "the Flathub manifest installs different files"
    );
    assert_eq!(module(&local)["buildsystem"], module(&flathub)["buildsystem"]);
}

/// What Flathub refuses: a source it cannot fetch. The tag is a label somebody
/// can move, so the commit is what makes the build repeatable — both are named,
/// and the placeholder commit has to be replaced before this is submitted.
#[test]
fn the_flathub_manifest_builds_from_a_published_address() {
    let flathub = flathub();
    let sources = flathub["modules"][0]["sources"].as_sequence().unwrap().clone();

    let code = &sources[0];
    assert_eq!(code["type"].as_str(), Some("git"), "{code:?}");
    assert!(code["url"].as_str().is_some_and(|url| url.starts_with("https://")));
    assert!(code["tag"].as_str().is_some_and(|tag| tag.starts_with('v')));
    assert!(code["commit"].as_str().is_some(), "a tag alone is not a repeatable build");

    // The crate list travels with it, and by name rather than by path: in a
    // Flathub repository the manifest and that file sit side by side.
    assert_eq!(sources[1].as_str(), Some("generated-sources.json"));
    assert!(!sources.iter().any(|source| source["type"].as_str() == Some("dir")));
}

/// The tag has to name a release that exists. `bump.sh` keeps it in step; this
/// is what notices when someone edits a version by hand instead.
#[test]
fn the_tag_matches_the_version_this_tree_is_at() {
    let flathub = flathub();
    let tag = flathub["modules"][0]["sources"][0]["tag"].as_str().unwrap();
    assert_eq!(tag, format!("v{}", env!("CARGO_PKG_VERSION")));
}
