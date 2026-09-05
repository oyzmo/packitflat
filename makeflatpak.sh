#!/usr/bin/env bash
#
# Build the Flatpak. BUILDS ONLY — nothing is installed, scratch state stays in
# build/, and the bundle lands in dist/.
#
#     ./makeflatpak.sh
#
# The source for this app is written on one machine and the Flatpak is built on
# another, so everything the build needs is in the repository: the manifest, the
# data files, and generated-sources.json (the full crate list, because a Flatpak
# build has no network). This script regenerates that list when Cargo.lock is
# newer, then preflights the things that have actually gone wrong before.
set -euo pipefail
cd "$(dirname "$0")"

APP_ID="no.oyzmo.PackItFlat"
MANIFEST="flatpak/${APP_ID}.yml"
RUNTIME_VERSION="50"
RUST_EXTENSION="org.freedesktop.Sdk.Extension.rust-stable"
BUILD_DIR="build"
REPO_DIR="build/repo"
DIST_DIR="dist"

need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "error: $1 is not installed." >&2
        [ -n "${2:-}" ] && echo "       $2" >&2
        exit 1
    }
}

need flatpak-builder "On Fedora: sudo dnf install flatpak-builder"
need python3
need flatpak

# The runtime and SDK have to be there; the build fails deep inside cargo
# otherwise, with an error that says nothing about the real cause.
for ref in \
    "org.gnome.Platform//${RUNTIME_VERSION}" \
    "org.gnome.Sdk//${RUNTIME_VERSION}" \
    "${RUST_EXTENSION}//25.08"; do
    if ! flatpak info "${ref%//*}" "${ref#*//}" >/dev/null 2>&1; then
        echo "error: missing ${ref}. Install it with:" >&2
        echo "       flatpak install flathub ${ref}" >&2
        exit 1
    fi
done

# Regenerate the vendored crate list whenever the lock file moved.
if [ ! -f generated-sources.json ] || [ Cargo.lock -nt generated-sources.json ]; then
    echo "==> regenerating generated-sources.json from Cargo.lock"
    python3 flatpak/cargo-sources.py Cargo.lock -o generated-sources.json
fi

# Paths inside the manifest resolve relative to flatpak/, not to the project
# root. This has bitten before: `path: .` hands the builder the flatpak
# directory and the build dies on a missing Cargo.toml. Fail here instead, where
# the message can say what is wrong.
python3 - "$MANIFEST" <<'PY'
import os, sys, re
manifest = sys.argv[1]
base = os.path.dirname(os.path.abspath(manifest))
text = open(manifest).read()

for path in re.findall(r'^\s*path:\s*(\S+)\s*$', text, re.M):
    resolved = os.path.normpath(os.path.join(base, path))
    if os.path.isdir(resolved):
        if not os.path.isfile(os.path.join(resolved, "Cargo.toml")):
            sys.exit(f"error: {manifest}: `path: {path}` is {resolved}, "
                     "which has no Cargo.toml")
    elif not os.path.exists(resolved):
        sys.exit(f"error: {manifest}: `path: {path}` does not exist ({resolved})")

for include in re.findall(r'^\s*-\s+(\S+\.json)\s*$', text, re.M):
    resolved = os.path.normpath(os.path.join(base, include))
    if not os.path.isfile(resolved):
        sys.exit(f"error: {manifest} refers to {include}, which is missing "
                 f"({resolved}). Run: python3 flatpak/cargo-sources.py")
print("    manifest paths check out")
PY

echo "==> building (this takes a while — every crate is compiled from scratch)"
mkdir -p "$DIST_DIR"
flatpak-builder --force-clean --disable-rofiles-fuse \
    --repo="$REPO_DIR" \
    "$BUILD_DIR/app" "$MANIFEST"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
BUNDLE="${DIST_DIR}/PackItFlat-${VERSION}.flatpak"
flatpak build-bundle "$REPO_DIR" "$BUNDLE" "$APP_ID"

echo
echo "built: $BUNDLE"
echo "install it yourself with:  flatpak install --user $BUNDLE"
