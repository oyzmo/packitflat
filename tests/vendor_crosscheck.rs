//! The crate list this app writes, checked against the script that has been
//! producing the same file for the sibling projects.
//!
//! Two independent implementations — one in Rust reading Cargo.lock line by
//! line, one in Python using tomllib — over this project's own lock file. If
//! they agree on every URL, checksum and destination, the Rust one is right.

use std::process::Command;

#[test]
fn the_generated_crate_list_matches_the_reference_script() {
    let lock = std::fs::read_to_string("Cargo.lock").expect("this project has a Cargo.lock");
    let ours = packitflat::vendor::cargo_sources(&lock).expect("the list is generated");

    let dir = tempfile::tempdir().unwrap();
    let reference = dir.path().join("reference.json");

    let run = Command::new("python3")
        .arg("flatpak/cargo-sources.py")
        .arg("Cargo.lock")
        .arg("-o")
        .arg(&reference)
        .output();

    let Ok(output) = run else {
        eprintln!("python3 isn't available; skipping the cross-check");
        return;
    };
    assert!(
        output.status.success(),
        "the reference script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let theirs = std::fs::read_to_string(&reference).unwrap();

    // Compared as data, not as text: the two write the same entries, and there
    // is no reason to insist they lay the whitespace out identically.
    let ours: serde_yaml_ng::Value = serde_yaml_ng::from_str(&ours).unwrap();
    let theirs: serde_yaml_ng::Value = serde_yaml_ng::from_str(&theirs).unwrap();

    let ours = ours.as_sequence().unwrap();
    let theirs = theirs.as_sequence().unwrap();
    assert_eq!(
        ours.len(),
        theirs.len(),
        "different number of entries: {} vs {}",
        ours.len(),
        theirs.len()
    );

    for (index, (ours, theirs)) in ours.iter().zip(theirs.iter()).enumerate() {
        assert_eq!(ours, theirs, "entry {index} differs");
    }
}

/// What the button on the dependencies step does, start to finish: read the lock
/// file, write the list, point the manifest at it, and leave a project that no
/// longer complains about downloading.
#[test]
fn preparing_a_real_project_leaves_it_ready_to_build_offline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy("Cargo.lock", dir.path().join("Cargo.lock")).unwrap();
    std::fs::copy("Cargo.toml", dir.path().join("Cargo.toml")).unwrap();

    let (mut project, _) = packitflat::project::Project::from_folder(dir.path());
    project.manifest.app_id = "no.oyzmo.Prepared".into();

    // Before: the app says so.
    let before = packitflat::validate::project(&project);
    assert!(
        before
            .iter()
            .any(|issue| issue.field == packitflat::validate::Field::Dependencies),
        "an unprepared Rust project is flagged"
    );

    let (written, count) = packitflat::vendor::prepare_cargo(dir.path()).unwrap();
    assert!(written.is_file());
    assert!(count > 50);
    packitflat::vendor::wire_in(&mut project.manifest, packitflat::vendor::Ecosystem::Cargo);

    // After: nothing left to say about it, and the manifest points at the list.
    let after = packitflat::validate::project(&project);
    assert!(
        !after
            .iter()
            .any(|issue| issue.field == packitflat::validate::Field::Dependencies),
        "{after:#?}"
    );
    assert!(packitflat::vendor::is_wired(
        &project.manifest,
        "cargo-sources.json"
    ));

    // And what was written is a list flatpak-builder could read.
    let text = std::fs::read_to_string(&written).unwrap();
    let entries: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).unwrap();
    assert_eq!(entries.as_sequence().unwrap().len(), count * 2 + 1);
}

#[test]
fn every_crate_in_the_lock_file_is_accounted_for() {
    let lock = std::fs::read_to_string("Cargo.lock").unwrap();
    let counted = packitflat::vendor::cargo_crate_count(&lock).unwrap();

    // Every [[package]] with a source line is a download; the ones without are
    // this project's own crates.
    let from_lock = lock
        .split("[[package]]")
        .filter(|block| block.contains("source = \"registry+"))
        .count();
    assert_eq!(counted, from_lock);
    assert!(counted > 50, "this project has plenty of dependencies");
}
