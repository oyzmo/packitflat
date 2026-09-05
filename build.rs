// Compiles the GResource bundle that carries the .ui files, the icon and the
// menus into the binary, so the app runs identically from the source tree and
// from the Flatpak with no install step.
//
// glib-compile-resources is called directly rather than through the
// glib-build-tools crate: every dependency has to be transcribed into
// generated-sources.json for the offline Flatpak build, and that crate does
// nothing but shell out to the same binary.

use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let target = Path::new(&out_dir).join("packitflat.gresource");

    println!("cargo:rerun-if-changed=data/packitflat.gresource.xml");
    println!("cargo:rerun-if-changed=data/ui");
    println!("cargo:rerun-if-changed=data/no.oyzmo.PackItFlat.svg");

    let status = Command::new("glib-compile-resources")
        .args(["--sourcedir", "data"])
        .arg("--target")
        .arg(&target)
        .arg("data/packitflat.gresource.xml")
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("glib-compile-resources failed with {s}"),
        Err(e) => panic!(
            "could not run glib-compile-resources ({e}). \
             On Fedora: sudo dnf install glib2-devel"
        ),
    }

    compile_schemas(&out_dir);
}

/// Compile the GSettings schema into OUT_DIR so a run from the source tree can
/// find it (main.rs points GSETTINGS_SCHEMA_DIR here in debug builds). In the
/// Flatpak the schema is installed properly and flatpak-builder compiles it;
/// this only exists so development doesn't silently fall back to the file store.
fn compile_schemas(out_dir: &str) {
    let schemas = Path::new(out_dir).join("schemas");
    println!("cargo:rerun-if-changed=data/no.oyzmo.PackItFlat.gschema.xml");

    if std::fs::create_dir_all(&schemas).is_err() {
        return;
    }
    if std::fs::copy(
        "data/no.oyzmo.PackItFlat.gschema.xml",
        schemas.join("no.oyzmo.PackItFlat.gschema.xml"),
    )
    .is_err()
    {
        return;
    }

    // Not fatal: without it the app uses its file-based preferences instead,
    // which is exactly what a machine with no glib tools should do.
    match Command::new("glib-compile-schemas").arg(&schemas).status() {
        Ok(status) if status.success() => {}
        Ok(status) => println!("cargo:warning=glib-compile-schemas failed with {status}"),
        Err(e) => println!("cargo:warning=could not run glib-compile-schemas ({e})"),
    }
}
