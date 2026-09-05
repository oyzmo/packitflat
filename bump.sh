#!/usr/bin/env bash
#
# Bump the version. Cargo.toml is the single source of truth — the welcome page
# and the About dialog read it through env!("CARGO_PKG_VERSION") — and this
# script carries the new number into Cargo.lock, the metainfo release list and
# the vendored crate list.
#
#     ./bump.sh patch|minor|major [-m "what changed"]
#
# Never edit a version string by hand.
set -euo pipefail
cd "$(dirname "$0")"

PART="${1:-}"
NOTE=""
if [ "${2:-}" = "-m" ]; then
    NOTE="${3:-}"
fi

case "$PART" in
    patch | minor | major) ;;
    *)
        echo "usage: $0 patch|minor|major [-m \"what changed\"]" >&2
        exit 2
        ;;
esac

CURRENT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ -n "$CURRENT" ] || { echo "error: no version in Cargo.toml" >&2; exit 1; }

IFS=. read -r MAJOR MINOR PATCH <<<"$CURRENT"
case "$PART" in
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    patch) PATCH=$((PATCH + 1)) ;;
esac
NEW="${MAJOR}.${MINOR}.${PATCH}"
TODAY=$(date +%F)

VERSION="$NEW" DATE="$TODAY" RELEASE_NOTE="$NOTE" python3 - <<'PY'
import os
import pathlib
import re

new = os.environ["VERSION"]
date = os.environ["DATE"]
note = os.environ["RELEASE_NOTE"] or "Maintenance release."

# Cargo.toml: only the [package] version, which is the first one in the file.
toml = pathlib.Path("Cargo.toml")
toml.write_text(re.sub(r'^version = ".*"', f'version = "{new}"', toml.read_text(), count=1, flags=re.M))

# Cargo.lock: the version inside our own [[package]] block.
lock = pathlib.Path("Cargo.lock")
if lock.exists():
    lock.write_text(
        re.sub(
            r'(\[\[package\]\]\nname = "packitflat"\nversion = ")[^"]*(")',
            rf'\g<1>{new}\g<2>',
            lock.read_text(),
            count=1,
        )
    )

# metainfo: a new <release> at the top of the list.
meta = pathlib.Path("data/no.oyzmo.PackItFlat.metainfo.xml")
entry = (
    f'    <release version="{new}" date="{date}">\n'
    f"      <description>\n"
    f"        <p>{note}</p>\n"
    f"      </description>\n"
    f"    </release>\n"
)
text = meta.read_text()
if f'version="{new}"' not in text:
    meta.write_text(text.replace("  <releases>\n", "  <releases>\n" + entry, 1))

# The Flathub manifest names the tag it builds. It is a copy kept for a
# submission rather than something built here, so nothing else would ever
# notice it going stale — and a stale tag in a submission builds the wrong
# version. The commit beside it is deliberately left alone: only the person
# making the tag knows it.
flathub = pathlib.Path("flatpak/flathub/no.oyzmo.PackItFlat.yml")
if flathub.exists():
    flathub.write_text(
        re.sub(r"^(\s*tag: )v[\d.]+$", rf"\g<1>v{new}", flathub.read_text(), flags=re.M)
    )
PY

# The crate list embeds this package's own version, and makeflatpak.sh only
# regenerates it when Cargo.lock is newer — which the edit above just made true,
# but do it here too so a bumped tree is always ready to hand over for building.
python3 flatpak/cargo-sources.py Cargo.lock -o generated-sources.json

echo "$CURRENT -> $NEW"
echo "Remember: this only prepares the release. Building and publishing are manual."
