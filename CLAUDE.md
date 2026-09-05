# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This App Is

Pack It Flat: a GNOME app (Rust + GTK4 `gtk4` crate aliased `gtk`, Libadwaita aliased `adw`) that
walks someone who has never written a Flatpak manifest through producing one — guided wizard or
editor mode over a shared in-memory model, generating `<app-id>.yml`, `.desktop`, AppStream metainfo
and icons, and optionally running `flatpak-builder` and translating its failures into plain
sentences. App ID `no.oyzmo.PackItFlat`, binary `packitflat`.

`docs/brief.md` is the full brief and every section of it is binding. Read it before adding a
feature. (`README.md` is the front page for people finding the project, not the spec.)
The one requirement that is easy to lose while coding: **assume the user knows nothing.** Every
input gets a label, a plain-English line under it and a realistic placeholder; no Flatpak jargon in
the UI (`finish-args` is "Permissions"); no flatpak-builder error reaches the screen without a
translation and a fix button.

**All six milestones of the brief are built**: welcome page, project model, YAML/JSON import, the
eight-step guided setup, editor mode with two-way raw YAML sync, offline dependency preparation,
permissions in plain language with a live risk summary, generation of the manifest, desktop entry,
metainfo and icon, the build runner with its preflight checklist, streaming log and translated
failures, and templates.

The brief's explicit non-goals still stand: no Flathub submission automation, no arbitrary
shell-script editing UI, no non-Flatpak packaging formats.

Deliberately deferred, each with a working stand-in rather than a dead end:

- **The runtime step lists and explains; the build page installs.** The wizard's step 2 offers the
  `flatpak install` line to copy; the build page's preflight has the button that runs it, because
  that is where the host-spawn plumbing lives.
- **Archive checksums are computed from a local copy, not downloaded.** Downloading would mean
  `--share=network` in this app's own manifest, which it deliberately doesn't ask for; the "From a
  file…" button hashes a copy the user already has with `sha256.rs`.
- **Node and Python dependency lists are not generated here.** `package-lock.json` doesn't carry
  what a Flatpak build needs (flatpak-node-generator reconstructs an npm cache layout), and
  `requirements.txt` needs PyPI looked up over the network. Producing a subtly wrong list would be
  worse than producing none, so those two get the tool's exact command and are wired in once the
  file exists.
- **Screenshots are addresses, not files.** AppStream fetches them over the web, so the metainfo
  needs URLs; uploading pictures anywhere is not this app's job.
- **One icon file, not a generated set.** SVG goes to `scalable`, a PNG to its own size directory.
  Rasterising an SVG into several PNG sizes would mean a image-processing dependency to vendor, and
  Flatpak needs only the one file — what actually goes wrong for beginners is the *path and name*,
  and those are computed in `icons.rs` rather than typed.
- **The editor's forms edit the app's own module.** A manifest with several modules lists them all
  in the sidebar, and the extra ones are written back untouched, but they are edited through the
  manifest text rather than through forms.
- **Nothing else.** The raw YAML pane is a real GtkSourceView (YAML highlighting, line numbers,
  Adwaita colour scheme following light/dark); `gtksourceview5-devel` is installed here and the
  library is part of the GNOME 50 runtime, so the Flatpak ships nothing extra for it.

## Decisions that supersede the brief

- **App ID `no.oyzmo.PackItFlat`**, name "Pack It Flat". The brief's `io.github.<user>.FlatpakStudio`
  and every "flatpak-studio" path in it are pre-domain placeholders: autosave lives under
  `$XDG_DATA_HOME/packitflat/projects/`.
- **Cargo, not Meson.** Meson isn't installed here and every sibling app of the user's is cargo plus
  a Flatpak manifest that calls it.
- **Claude builds and tests the source; the user builds the Flatpak.** Don't run `./makeflatpak.sh`
  and don't start a full `flatpak-builder` run: packaging is the user's job, and a real build of
  this app pulls the GNOME SDK and the rust-stable extension — well over a gigabyte — which is not
  something to begin without being asked. The corollary is that the repository must stay
  self-sufficient: manifest, data files and `generated-sources.json` complete and consistent, so
  `./makeflatpak.sh` works on a machine that has never seen this code.
- **flatpak-builder itself is installed here (1.4.10), and the failures worth checking are cheap.**
  A build against a runtime this machine hasn't got fails at once, before anything is downloaded,
  and a manifest that doesn't parse fails before that — so the *wording* `build::diagnose` matches on
  can be taken from the tool rather than from memory. Two of its rules now carry flatpak-builder's
  exact output. That is worth doing whenever a rule is added; what it does not license is a real
  build.
- **`.ui` files, not Blueprint** — `blueprint-compiler` is absent here and would be an extra build
  dependency on the Flatpak side.
- **The app follows the system colour scheme** (asked and answered; this overrides the user's
  standing "no dark mode" rule for this app only). `src/style.rs` carries both palettes.
- **Strings are marked, gettext is not wired up yet.** Every user-visible string in Rust goes
  through `i18n::t()`; `.ui` strings carry `translatable="yes"`. When a catalogue is added, `t()`
  becomes the gettext call and nothing else changes.

## Publishing

**This project is public, and it is the exception to the user's "no git" rule** — asked for and
answered on 2026-09-05. It lives at **https://github.com/oyzmo/packitflat**, GPL-3.0-or-later, first
commit `0391c259` at version 0.20.1.

Git runs on the user's **host machine**, in `~/Code/rust/packitflat` — the same files as this share,
which cannot hold a repository (`git init` fails on virtiofs). **Claude does not run git here; the
user runs it there.** Commits are authored as `oyzmo <74640760+oyzmo@users.noreply.github.com>`:
neither the user's real name nor their private address goes into a public commit, and that is
settled, not a preference to re-ask about.

`.gitignore` is what keeps the repository at 1.6 MB instead of 500 MB. Its patterns are anchored
(`/build/`, leading and trailing slash) because a bare `build` would swallow `build.rs` and
`src/build.rs`, which are real source. `dev/` is out; **`docs/screenshots/` is in** — the metainfo's
`<screenshots>` point at its raw GitHub URLs, so an over-broad ignore rule there breaks a Flathub
submission with no local symptom.

Where the Flathub submission stands, and what is left of it, is in the header of
`flatpak/flathub/no.oyzmo.PackItFlat.yml` — six numbered steps with the exact commands. The state as
of the last session: pushed to GitHub; **not yet tagged `v0.20.1`**, and the manifest's filled-in
`commit:` is not yet committed. Do those in that order, so the tag stays on the commit the manifest
names.

## Commands

The project sits on a **virtiofs share**, which is unreliable for parallel compiler writes, so the
build directory has to be elsewhere — and not in `/tmp`, which is an 8 GB tmpfs this project fills
(debug plus release plus test binaries came to 6.4 GB and hit the quota mid-run). `~/.cache` is on
the 78 GB disk:

```sh
export CARGO_TARGET_DIR=~/.cache/packitflat-target   # required
cargo build
cargo test
cargo test round_trips                           # single test by name substring
cargo clippy --all-targets                       # clean; keep it that way
```

Build deps on Fedora: `sudo dnf install gtk4-devel libadwaita-devel gtksourceview5-devel glib2-devel`.
`git init` fails on this share, and the user's rule is no git anyway.

`./bump.sh patch|minor|major [-m "note"]` is the only way the version changes: `Cargo.toml` is the
source of truth (`env!("CARGO_PKG_VERSION")` feeds the welcome page's version ticker and the About
dialog), and the script carries it into `Cargo.lock`, the metainfo `<releases>` list and
`generated-sources.json`. Never edit a version string by hand.

`./makeflatpak.sh` builds the Flatpak and exports a bundle to `dist/` without installing anything —
**it is for the user's other machine; don't run it.** It preflights the runtime, the SDK, the
rust-stable extension and the manifest's relative paths, and regenerates the crate list first.

`PACKITFLAT_DEV_WIZARD=3` opens the wizard at that step (1-based) — the screenshot harness cannot
click its way there. `PACKITFLAT_DEV_EDITOR=yaml` opens the editor at a pane
(`manifest`/`sources`/`build`/`permissions`/`yaml`/`problems`), and `PACKITFLAT_DEV_DIALOG=mode` or
`=preferences`/`=templates`/`=clone`/`=licence` shows a dialog. `PACKITFLAT_DEV_BUILD=ready|run|ok|fail|check` opens the build page,
with a stand-in build where the mode asks for one. `PACKITFLAT_DEV_GEOMETRY=1` prints the widget
tree with each widget's size, position and **minimum** width, which is how layout questions get
settled; a bigger number (`=15`) goes that many levels deep, and the widgets that overlap are never
near the top. `PACKITFLAT_DEV_NARROW=360` asks the page on top how narrow it could be and fails if
that is wider than a phone.
`PACKITFLAT_DEV_LICENCE=gplv3` opens the licence picker from its row, types that, chooses the first
match and prints what the project was left holding — end to end, because both halves of that path
have been broken before. `PACKITFLAT_DEV_AUTOSAVE=1` types into a step, presses nothing, and checks
that the saved copy caught up on its own.

**Every hook that edits anything needs a project, and none of them can open one** — the wizard, the
editor, the licence picker, the autosave and switch checks, the build page. Give the app a path:
`PACKITFLAT_DEV_AUTOSAVE=1 packitflat dev/probe/hello/no.oyzmo.Hello.yml`. Without one they now say
so and exit 1 with that example in the message; `PACKITFLAT_DEV_EDITOR` used to hang instead, which
looked like a wedged app rather than a missing argument. Pass the **manifest**, not the folder: a
folder is opened as a new project with no app ID, so there is no manifest to name and the build
page has no command — a state worth testing on purpose, not by accident.

`PACKITFLAT_DEV_SHOT=/path/out.png` renders the window to a PNG and quits — GNOME refuses
non-interactive screenshots over D-Bus, so `src/devshot.rs` snapshots the window through its own GTK
renderer instead. `PACKITFLAT_DEV_SIZE=360x780` resizes first, which is how the phone layout the
brief requires gets checked. Both are behind `debug_assertions` and absent from release builds.
Screenshots land in `dev/`, which never ships (it's in the manifest's `skip:` list and
`Cargo.toml`'s `exclude`).

`packitflat <manifest.yml|folder>` opens that project directly — the app declares `HANDLES_OPEN`, so
this is also what the `.desktop` entry's `%f` and "Open with" use, and it is the quickest way to
exercise the import path without clicking through a file chooser.

## Architecture

**The library decides, the binary displays.** `src/lib.rs` exposes `manifest`, `project`, `detect`
and `i18n`; the binary (`main.rs`, `window.rs`, `style.rs`, `devshot.rs`) consumes them as
`packitflat::…`. The split exists so everything that decides anything is testable without a display,
and so wizard mode and editor mode are provably working on the same model — the brief's "never lose
data when switching" is only guaranteed if there is nothing to sync.

- `manifest.rs` — the Flatpak manifest as typed data, plus import and export.
- `validate.rs` — every rule, each carrying *what* is wrong and *what to type instead*, plus the
  step it belongs to so the review page can offer "Go there". Errors block generating; warnings
  never do.
- `generate.rs` — the plan (where the file goes, what it would replace) computed and shown before
  anything is written, then an atomic write through a temp file and a rename.
- `runtimes.rs` — the runtime catalogue: parsing `flatpak remote-ls` output, the cached copy, the
  bundled fallback, friendly names, and the dated end-of-life table. Text in, text out; running
  flatpak is the UI's job.
- `spdx.rs` — the licence list, searchable by identifier, everyday name and common misspelling.
- `sha256.rs` — hand-rolled FIPS 180-4 so a checksum needs no dependency crate.
- `vendor.rs` — what a project downloads while building, written down so the build doesn't have to.
  **The Rust crate list is produced natively**: every crate's address and sha256 is already in
  `Cargo.lock`, so it is a transcription needing no network, no Python and no
  flatpak-builder-tools. Node and Python can't be done that way — their lock files don't carry
  enough — so those get the exact command and the file is wired in once it exists.
- `templates.rs` — six starting points, each a project that would build if you gave it code.
- `git.rs` — starting from a pasted repository address: what the address means (a `tree/main/src`
  link is trimmed back to the repository and the user is told), where the code may go (an occupied
  folder is refused, never merged into), the clone command, and what a git failure was about.
- `build.rs` — which command to run, whether the computer is ready to run it, and what a failure
  means. **Nothing here starts a process**: it is text in, text out, so every command line, every
  preflight rule and every error translation is tested without a build ever happening.
- `permissions.rs` — `finish-args` as switches, and the **risk summary**: what the app would be able
  to do, said in the present tense about the app rather than about the flags, worst first, with the
  portal alternative where there is one. The safety-critical module; every rule in it is tested.
- `appdata.rs` — the desktop entry and the AppStream metainfo as text, plus the category list.
- `oars.rs` — the age rating as six plain questions, mapped to OARS identifiers afterwards.
- `icons.rs` — what the chosen picture actually is (by its bytes, not its extension), where it
  belongs, and what is wrong with it before the build rather than after.
- `sync.rs` — when the raw YAML view may be rewritten from the model and when it may not. The whole
  point of the module is that this decision is testable away from a widget.
- `settings.rs` — the remembered preferences. GSettings when its schema is installed, a file under
  the config directory when it isn't; **the schema is looked up first, because `Settings::new` on a
  missing schema aborts the process** and there is no catching it.
- `project.rs` — a source folder plus its manifest, and the autosave store under
  `$XDG_DATA_HOME/packitflat/projects/` (writes through a temp file and a rename, because the whole
  point is surviving an interruption). Inside the Flatpak, `XDG_DATA_HOME` already points at the
  app's own data directory, so the same code is right in both places.
- `detect.rs` — what kind of project a folder holds, with the *sentence* explaining the conclusion.
  Marker order matters: Meson and CMake are checked before `Makefile`, because both generate one.
- `window.rs` — the welcome page and the project summary. The `imp` module is template plumbing
  only; behaviour lives in the `impl PifWindow` block.
- `wizard.rs` — the five guided steps, as one `AdwNavigationPage` subclass over an `AdwCarousel`.
- `editor.rs` — editor mode: sidebar of parts, a pane for each, and the manifest as text.
- `forms.rs` — everything the two modes must not implement twice: the `Handle`, the source rows, the
  build-system list, the issue rows, the write-the-files flow.
- `panes.rs` — the two largest shared forms: what the app may do, and how it looks. Guided step 5
  and 6 and the editor's matching panes are the *same widgets* built into a box each mode supplies.
- `build_page.rs` — the build page: checklist, switches, streaming log, and what happened.
- `clone_page.rs` — the "Start from a Git address" dialog: the form, the running clone, the failure.
- `proc.rs` — running other programs on the GLib main loop. `output` for short questions, `stream`
  for a build: stdout and stderr are **merged on purpose**, because two streams interleaved out of
  order are worse than useless in a log someone is trying to read.

### Starting from a Git address

The code is cloned **shallowly, once**, because the app has to look at it: detection reads the
folder, the crate list is read out of `Cargo.lock`, the program's name comes from `Cargo.toml`.
Afterwards `git::use_repository` replaces the folder source with a git source **pinned to the commit
that was actually checked out** — a manifest naming a branch builds something different every day,
and pinning is the whole promise of a repeatable build. The clone stays as the project folder, so
the manifest and the files beside it are written into it.

`PACKITFLAT_DEV_CLONE=<url> PACKITFLAT_DEV_INTO=<folder>` runs the whole thing against a real
repository and prints a pass/fail line per rule. It has been checked against `octocat/Spoon-Knife`
(nothing to detect) and `sharkdp/hexyl` (Rust: detected, 67 crates found in its lock file).

### Two bugs worth not reintroducing

**A saved project is named after its folder, not its app ID.** It used to be the app ID, so filling
that in later saved a second copy under a new name and left the first on the welcome page — going
between the two modes could leave a trail of them. `recents_in` also collapses entries that share a
folder, so copies written by older versions show once.

**An `AdwPreferencesGroup`'s `first_child()` is its own internal box, never a row.** The obvious
"remove all the rows" loop therefore stops immediately and every rebuild *appends* another set. In
the editor's manifest pane that left the stale first set — built before anything had been typed — at
the top of the page, looking exactly like the app had forgotten what was entered. Rows built in code
are kept in a `Vec` and removed by handle.

### The build runner

The build runs `cd <project folder> && flatpak-builder …` through `sh -c`, because every path in a
manifest is relative to the folder holding it. Inside the sandbox the whole command is wrapped in
`flatpak-spawn --host` — a Flatpak cannot build a Flatpak — and when that wrapper doesn't work, the
preflight says so plainly and the command is offered to copy instead of the button being left to
fail.

**A `simple` module installs exactly what its build-commands say, and nothing else.** `detect` and
`templates` suggest commands that build the program and install *the binary* — so the finished
Flatpak had `bin/` and `share/licenses/` and nothing more: no menu entry, no icon, no AppStream. The
build succeeds, the app installs, and the only symptom is a generic icon in a Flatpak manager and no
entry in the menu. (A real build did exactly this; the GNOME icon the user saw came from a separate
host install.) `generate::sync_install_commands` puts the three `install -Dm644` lines in, from the
same plan the review step shows, and runs in both modes' `collect` so the preview and the written
file agree. Lines are matched by their *destination*, so it never adds one twice and editing the
left-hand side keeps working; unticking a file on the review step takes its line out with it. CMake,
Meson and autotools modules are skipped — there the project's own `install()` rules do this, and
`validate` says so when they are missing.

**And that fix did not reach the manifests it was written for, because it only ran from `collect`.**
`collect` runs when a *widget changes*. So opening an existing manifest that was missing the lines
and pressing "Write the manifest" without touching a build field wrote it back exactly as broken as
it arrived — which is what happened to a real project packaged with 0.16.0, the very release that
added the lines. Its bundle held `bin/namp` and a licence, nothing else. `sync_install_commands` now
also runs in `window::open_project`, the one path into the project view, so a manifest is put right
as it is opened and the review step's preview is telling the truth before anything is pressed.

**flatpak-builder's default build system is `autotools`, not `simple`.** `Project::from_folder`
used to omit the `buildsystem:` line whenever it would have said `simple`, on the assumption that
simple was the default. It is not: a Rust module with the commands to build it and no build system
named is handed to autotools, which checks the code out and stops with `Can't find autogen,
autogen.sh or bootstrap` — a sentence about a file the project never had, naming nothing the user
did. The guided steps hid it, because their `collect` sets the build system from the list, so only a
project written without visiting that step came out broken. Every detected kind now names its build
system, `simple` included; `validate` makes it an error when a module has commands and no build
system; and `build::diagnose` translates the autogen line. (Found by building this app's own
repository from a manifest this app wrote.)

**A build that fetches its code cannot see the files written next to the manifest.** With a `dir`
source the build is handed the project folder, so the desktop entry, metainfo and icon are simply
there — which is why this never showed up until "Start from a Git address" was used in anger. With a
git or archive source the build fetches the code from somewhere else, and those three files exist
only on the user's computer: the build compiles everything, then stops on `install: cannot stat
'app.desktop'`. `sync_install_commands_with` now adds each of them as a `type: file` source when the
module has no `dir` source, with `dest:` for the icon so it lands in
`icons/hicolor/scalable/apps/` where its install line expects it. Both halves were measured against
flatpak-builder 1.4.10 before being written: a `file` source does travel with a git source, and
without `dest` the icon lands at the top and the install line misses it.

**The sync has to happen when the files are written, not when a build widget changes.** Twice now
the same shape of bug: `sync_install_commands` ran only from `collect`, which fires on a *build*
widget, so anything that changed the plan by another route left the manifest behind. The one that
reached a user: choosing an icon on the appearance step put the icon in the project folder and left
the manifest without the line installing it, so the build printed `WARNING: Icon referenced in
desktop file but not exported` and the app arrived with a blank icon in every Flatpak manager.
`forms::write_files` now syncs against **the plan it is about to write**, which is the only moment
that is reliably right — and being plan-shaped, it takes a switched-off file's line out at the same
time. Keep the `collect` calls too; they are what makes the review step's preview honest before the
button is pressed.

**An icon is looked for on every edit, not only when the project is opened.** The icon file is named
after the app ID, so a project started from a *folder* has nothing to find at open — the ID is still
empty — and by the time it is typed nobody would look again. `icons::adopt_existing` is called from
`open_project` and from both modes' `collect`; it never overrules a picture the user chose.

**An icon the app itself put in the project was forgotten the moment the project was reopened.** The
manifest has nowhere to record which picture was chosen, so `icon_source` came back `None`, the icon
dropped out of the plan, and with it went the line that installs it. `icons::existing` reads the
answer off the disk instead: an icon at `icons/hicolor/**/apps/<app-id>.svg|png` — the layout this
app writes — is adopted on open, scalable first, otherwise the largest PNG.

**Warehouse — and any manager like it — finds the icon through the icon theme, not through
AppStream.** Its source adds every installation's `exports/share/icons` to a `Gtk.IconTheme` and
looks up the app ID. So the icon in a Flatpak manager depends on exactly one thing: an icon
installed as `/app/share/icons/hicolor/<size>/apps/<app-id>.<ext>`, which flatpak then exports.
Neither the metainfo nor the app-info data has anything to do with it. That is worth knowing before
trading anything away to get a listing.

**Installing the metainfo puts `appstreamcli compose` in the build, and it refuses three things.**
flatpak-builder hands it whatever metainfo it finds installed, and a listing with no summary, no
description or no category is rejected — which fails the *whole build*, at the finish stage, after
everything has compiled, with a one-word code (`metainfo-no-summary`, `description-missing`,
`no-valid-category`). Each was reproduced on its own against flatpak-builder 1.4.10 and the GNOME 50
SDK, one field at a time, on the toy project in `dev/probe/`. So the app does not fail the build over
it and does not make those fields blocking either: `appdata::listing_is_complete` decides whether the
metainfo gets an install line, the review step's row says "written, but left out of the build until…"
when it doesn't, and `validate`'s advice says what that costs. A hand-edited manifest that hits it
anyway gets `build::diagnose` naming the missing field and a button to go there, rather than the
name of a program the user has never heard of.

**A build leaves a `.flatpak` file behind, and that is not optional-by-default.** Someone who
presses "Build it" wants the app, not a `build-dir`. `Options::make_bundle` is on by default, which
puts `--repo=build-repo` on the build command — the repository has to be written *during* the build,
there is no way to produce the bundle from `build-dir` afterwards — and runs `flatpak build-bundle`
in the same shell line, so the log reads as one job. The finished page names the file. Switching it
off brings back the old "Make a single file to share" action afterwards.

**A failure never arrives as a log.** `build::diagnose` matches the failures beginners actually hit
— missing runtime, no Flathub remote, the build reaching for the network, a changed checksum, a
filename mismatch, a full disk — and returns a sentence, the reason, the few relevant log lines, and
where possible a button that fixes it. Anything unrecognised still gets a sentence and the last 25
lines, with the whole log one expander away.

**The order of those matches is load-bearing, and `install` comes before the download checks.** A
CMake project that fetches anything prints a `FetchContent.cmake` policy warning in *every* build,
including the ones that get all the way to the end — so matching the bare word first blamed a
blocked download for a build that had already compiled fine. `find_failing` exists for that: the two
module names (`fetchcontent`, `externalproject`) only count on a line that also says error or
failed. (A real build hit exactly this.)

**A build that compiles everything and then says `unknown target 'install'` is not a Flatpak
problem.** flatpak-builder always runs the install step, and a `CMakeLists.txt` with no `install()`
rule has nothing for it to run. `detect::declares_install` reads the answer out of the project's own
build files — every `CMakeLists.txt`/`meson.build` down to three levels, skipping generated output —
so `validate` can say it before the build rather than after, with the lines to add. It answers
`None` for makefiles on purpose: an install target can hide behind an include or a recursive `make`,
and a false alarm here would be worse than silence.

**An SDK extension is two things, and the second one is easy to forget.** Switching on Rust adds
`org.freedesktop.Sdk.Extension.rust-stable` to `sdk-extensions`, which installs the compiler at
`/usr/lib/sdk/rust-stable/bin` — **and Flatpak does not put that on PATH**. Without
`build-options: append-path:` the build downloads every crate and then dies with `cargo: command not
found`, which looks like anything but a missing path. (A real build hit exactly this.)
`runtimes::sync_build_paths` keeps `append-path` and `prepend-ld-library-path` in step with whatever
extensions are switched on, preserving any entries the user added; it runs when an extension is
toggled, when a folder is detected, when a template is applied, and when a project is opened in
either mode. `validate` warns if a manifest ever arrives without it, and `build::diagnose` names the
missing program rather than blaming flatpak-builder.

**"Open a terminal" is a table, not a command line.** Terminals agree on almost nothing —
`--working-directory=` then `--`, or `--workdir` then `-e`, or nothing at all — so `build::TERMINALS`
lists them in preference order with the style each wants, and the page offers the first one this
computer has, by name ("Open Terminal", "Open Konsole"). The script it runs ends in a `read`, so the
window stays open after the build instead of vanishing with the error in it.
`PACKITFLAT_DEV_BUILD=terminal` prints the exact command line it would run.

**Flatpak has two installations, and a remote in one is invisible to a command aimed at the other.**
This is the single worst bug the app has had, because everything about the computer was fine. Fedora
sets Flathub up **system-wide** and has no user installation at all; the preflight asked only whether
a remote called `flathub` existed *anywhere*, ticked "Flathub is set up", and then offered a download
with `--user` on it — which fails with `No remote refs found for ‘flathub’`. A green tick above a
button that cannot work.

Worse, `--install-deps-from=flathub` has the same problem inside flatpak-builder: it acts on it by
running `flatpak --user install -y --noninteractive flathub …`, in the build's own installation. So
on that computer the app's *default build command* died with the SDK already installed and sitting
right there. Reproduced, and then fixed and re-reproduced:

```
Dependency Sdk: org.gnome.Sdk 50
Installing org.gnome.Sdk/x86_64/50 from flathub
error: No remote refs found for ‘flathub’
```

`build::Scope` is the answer: `remote_scope` reads `flatpak remotes --columns=name,options` (the
options column carries `user` or `system`), and it is threaded through `install_refs_command`,
`build_command`, `export_commands` and `diagnose`. The rules:

- Build with `--user` always. It needs no password, and **finding** the runtime is not scoped —
  a `--user` build uses a system-wide SDK without complaint. Measured, not assumed.
- Add `--install-deps-from=flathub` **only** when Flathub is a user remote. Everywhere else the
  checklist's own "Download them" does it first, with the right flag.
- The check says so when Flathub is system-wide, because that download will ask for a password, and
  an unexplained password prompt is the sort of surprise this app exists to prevent.

The generated manifest's header used to document the broken combination for people building by hand.
It now documents `flatpak-builder --force-clean --user build-dir <file>` — the form that works
anywhere — and explains `--install-deps-from` as the conditional extra it is.

**"Copy this and run it" has to work where it is pasted.** The three checks that offer to install a
missing package used to hand out `sudo dnf install …` on every computer, which for the person this
app is written for is worse than no suggestion. `build::package_command` reads `os-release` — the
host's at `/run/host/os-release` first, because inside a Flatpak the one at `/etc` is the runtime's
— and maps `ID`/`ID_LIKE` to dnf, apt, pacman, zypper, apk or emerge. `ID_LIKE` is what catches the
derivatives people actually run (Mint says `ubuntu debian`, Nobara says `fedora`). An unrecognised
distribution gets **no command at all**, and the check names the package instead.

**The build command row is a sentence when there is nothing to copy.** The manifest is named after
the app ID, so before one is filled in the command ended in a bare `.yml` — a line that looks
copyable, runs, and fails on a file that was never going to exist. It now says why it isn't there
yet, and the copy and terminal buttons go insensitive. Both of those read the command from
`imp.command_text`, never off the row's subtitle, which is prose in that state.
`PACKITFLAT_DEV_BUILD=terminal` used to build its argv around that empty command anyway and print
`sh -lc cd '…' && ;` — a shell syntax error — under an "ok". It now checks the thing that actually
protects the user in that state: that both buttons are insensitive.

**Two of the translations are checked against the tool's own words, not against remembered ones.**
`the_real_tools_missing_sdk_output_is_diagnosed_too` and
`a_manifest_that_doesnt_parse_is_named_rather_than_quoted` carry logs copied verbatim out of
flatpak-builder 1.4.10. The first one mattered: the real missing-SDK failure leads with
`error: org.gnome.Sdk/x86_64/50 not installed`, a line the hand-written test never had, and
`diagnose` reads from the top. Both failures happen before anything is downloaded, so reproducing
them costs nothing — `flatpak-builder --force-clean --user build-dir <manifest>` in a scratch folder
under `dev/`.

**A real build has now been run, end to end, and it is what found the Flathub-scope bug.**
`org.gnome.Sdk//50` is installed on this machine (system-wide), so a whole build takes about a
minute against a toy project in `dev/probe/hello`: a `simple` module, two `install` lines, no
compiler. It went through `flatpak-builder … --repo=build-repo --user build-dir` and
`flatpak build-bundle`, and left a `.flatpak` file whose name `bundle_file` predicts exactly. That
run is also where `status_line` was checked against every phase a real log prints — two of them,
`Running:` and `Finishing app`, said nothing at all, and `Running:` is most of what a simple module
prints while it builds.

Building the toy project again is cheap and worth doing after touching `build_command`,
`status_line` or `diagnose`. Building **this** app is still not: it wants the rust-stable extension
and a full cargo build, and packaging is the user's job.

`PACKITFLAT_DEV_BUILD=check` still drives the page with a stand-in build, because what it checks is
the *page* — that output streamed in, that the page moved on, that what landed on screen was the
sentence and not the log line.

### How the two modes stay honest

The window, the wizard and the editor share **one** `Rc<RefCell<Project>>`, wrapped in a
`forms::Handle`; none of them keeps a copy. That is what makes the brief's "never lose data when
switching" true by construction rather than by syncing. Switching modes is a `pop` followed by a
`push` of the other page over the same handle — the departing page records which button was pressed
(`wanted_editor` / `wanted_guided`) and the window acts on it in `connect_popped`.

Every change runs the same two-phase cycle: `collect` reads widgets into the project, `refresh`
re-derives everything shown from it. `Handle::silently` mutes writes while widgets are being filled
from the project, which is the only thing standing between this and an infinite loop.

**The saved copy is written by the handle, a moment after the last change.** It used to be written
only when the page changed or the mode was switched, so whatever was typed on the page you were on
was exactly what an interruption took — and surviving an interruption is the only reason the saved
copy exists. `Handle::write` now calls `save_soon`, which resets a 1.2-second wait rather than
queueing another save, so typing a sentence writes the file once at the end of it. A mode that edits
opts in with `Handle::new(…).autosave()`; the build page holds a handle and edits nothing, so it
doesn't. `save_now` is for the moments where waiting would be wrong — leaving a page, switching
mode, writing the files — and it cancels any wait still running.
`PACKITFLAT_DEV_AUTOSAVE=1` types into a step, presses nothing, and checks that the file on disk
caught up; it deliberately navigates nowhere, because navigating is what used to do the saving.

The raw pane is a `GtkSourceView` — a `GtkTextView` with extras — so the sync wiring is the same
code it was when it was a plain text view. `setup_highlighting` sets the YAML language and picks
`Adwaita` or `Adwaita-dark` from `AdwStyleManager`, following the desktop the way the rest of the app
does. **GtkBuilder resolves types by name**, so `sourceview::View::static_type()` is called at
startup before the editor's `.ui` is parsed; without it the template fails to load.

**The licence picker is a dialog this app builds, because `AdwComboRow`'s search cannot be made to
work.** It was one for a while, reaching through the popup for libadwaita's private `GtkStringFilter`
to set an expression and a substring match mode, and it was broken in two separate ways — both
measured with `PACKITFLAT_DEV_LICENCE`, neither visible from the code:

- libadwaita **clears the popup's search box** before what was typed reaches the filter. The signal
  trace is `changed "gplv3"`, then `changed ""`, then `search-changed ""`, and the filter is left
  holding `""` — so the list never narrows however the filter is set up. Poking the same filter
  directly with "gplv3" left 3 of 30 rows, which is how it was pinned on the entry rather than the
  expression.
- The position the row reports back is an index into the **filtered** list, while the list it is
  looked up in is the unfiltered one. So even with the search fixed, picking a licence after typing
  would have stored a different licence — silently, into the metainfo an app store reads.

`forms::choose_licence` replaces all of it: an `AdwDialog` with a search box and a list rebuilt from
`spdx::search` on every keystroke. That function is unit-tested and already knows "gplv3" means
GPL-3.0-or-later, the rows have room for the sentence saying what each licence allows, and nothing
depends on libadwaita's internals. Both modes use it — guided step 1 and the editor's manifest pane,
where it replaced a box to type an SPDX identifier into. `PACKITFLAT_DEV_LICENCE=<query>` drives the
whole path: it opens the picker from the row, types the query, activates the first row that comes
back and checks **what the project was left holding**, which is the half that used to be wrong.

**Every "What is this?" row scrolls itself into view when opened.** `forms::hook_expanders` walks a
page after it is built and connects every `AdwExpanderRow`; an explanation that unfolds below the
bottom of the window is one nobody reads, and these rows are what the whole app is built around.
Call it again after rebuilding a pane — it marks the rows it has already hooked, so calling it twice
is harmless. `PACKITFLAT_DEV_EXPAND=1` opens the first explanation on the visible page and checks
that the window actually moved.

**"The visible page" is the load-bearing word in that check, and it wasn't true.** A carousel keeps
all eight steps as children and the editor's stack keeps every pane, so the walk that looked for the
first `AdwExpanderRow` found step 1's explanation whichever step was open — eight steps reporting the
same 522px scroll, which is what gave it away. `visible_expanders` now descends only into a
carousel's current page and a stack's visible child, and skips hidden widgets; the check prints
which explanation it opened, because "the first one on this page" is precisely what was being got
wrong. It also waits for the carousel to *stop moving* first: `go_to_step` scrolls with an animation
that does not start until the carousel has been allocated, so a check that runs straight away reads
position 0 however long ago the step was asked for. It then opens **every** row on the page, one at
a time, closing each before the next — checking only the first meant checking the source row or the
category list on the pages where the explanation is the last row. A page with none at all is
reported as skipped rather than as a failure, and a row with no scrolling ancestor (the raw YAML
pane keeps its explanation above the editor) is measured against the window instead. With
`PACKITFLAT_DEV_SHOT` set it still stops at the first row and leaves it open, because that is the
state worth photographing.

**Every page now has one, and three of them didn't.** Guided steps 5, 6 and 8 and the `manifest`,
`build`, `dependencies`, `permissions`, `yaml` and `problems` panes had no explanation at all; step
4, step 7 and the `sources` pane had an expander that was about one field (the manifest preview, the
category list, a source row), so the page as a whole was never introduced and the old check was
satisfied by the wrong row. `forms::explainer(question, answer)` builds the row for the panes that
are built in code — `panes::build_permissions`, `build_dependencies` and `build_appearance`, which
is why guided steps 5, 6 and 7 and the editor's matching panes each got theirs from one place — and
the rest are in `wizard.ui` and `editor.ui` beside the page they explain. The answers say what the
page is *for* in the app's own voice; `use-markup` is off on every one of them, because they are
sentences and an ampersand in one would otherwise be swallowed.

**The raw YAML pane never overwrites text that doesn't parse.** `packitflat::sync` owns that
decision: `apply_text` writes to the model only on a successful parse, and `should_replace` refuses
to rewrite the view whenever the text already means what the model says — reformatting somebody's
spacing under their cursor is its own kind of clobbering. `PACKITFLAT_DEV_EDITOR=yaml
PACKITFLAT_DEV_SYNCTEST=1` drives the real buffer and prints a pass/fail line per rule, which is how
the wiring around that policy is checked.

Dynamic rows — sources, SDK extensions, the issue list — are built in code because their shape
depends on the data; everything static is in `data/ui/wizard.ui`. Editing a field inside a source
writes straight into that source and refreshes; adding or removing one rebuilds the list, because
the row closures capture indices.

**`AdwCarousel` gives each page only its natural width unless the page sets `hexpand`** — without
it, the next step shows through beside the current one. Measured, not guessed: `PACKITFLAT_DEV_GEOMETRY=1`
prints the widget tree with sizes and positions.

**The editor's minimum width was 676px, and the split view was only half of it.** An
`AdwNavigationSplitView` keeps both halves side by side however narrow the window gets, so its
minimum became the window's; an `AdwBreakpointBin` around it collapses it below 640px, and the bin's
own `width-request` is what lets the window shrink that far at all — it measures itself, not its
child. Two more things then had to give: the header buttons became `AdwButtonContent`, so a
breakpoint setter can take the words away and leave the icon (a plain `label` swap would have
destroyed the child), and the panes `GtkStack` had to be told `hhomogeneous=False`, because a stack's
minimum width is the widest page's *for every page* — one wide pane was the floor under the whole
window and nothing on screen said which.

The last 5px were a `GtkRevealer`: **a collapsed `AdwExpanderRow` still demands the width of the rows
it is hiding.** The source's "Path" row — an entry with `width-chars=18`, minimum 180px, and a
"Choose…" button at 104px beside it — was setting the floor for the whole editor while invisible.
`width-chars` is the *minimum*; `max-width-chars` is what decides how wide the field looks when there
is room. `forms::entry_row` now sets 8 and 18, so the field is unchanged in a wide window (`hexpand`
fills it out anyway) and gives way in a narrow one. Every pane now fits: 246px, or 275px for
`sources`, against 360.

Three guesses were wrong before that — the group title, the header buttons, the stack — and each
cost a build. The reason is that `PACKITFLAT_DEV_GEOMETRY` printed *allocations*, and an allocation
cannot tell a widget squeezed to 341px from one insisting on 341px. It now prints `min=` and `nat=`
too: follow `min` down the tree and the chain where it stops shrinking is the widget doing it. That
found it in one pass. `PACKITFLAT_DEV_NARROW=360` is the check that keeps it fixed — it resizes the
window first, because a breakpoint applies on allocation and measuring before that reports the
uncollapsed 700px — and pairs with `PACKITFLAT_DEV_EDITOR=<pane>`, which is the only way to reach one
pane at a time now the stack measures just the visible one.

**`AdwCarouselIndicatorDots` measure themselves from the carousel's *snap points*, and a carousel has
none until it has been allocated once.** So the bottom bar is laid out while they still claim the
width of a single dot — 27px measured — and they then draw all eight straight over the status
sentence beside them. `imp.dots.connect_map` queues one resize on the next idle, after which they
measure 132px and the sentence starts clear of them. Below 560px the two cannot both fit, so an
`AdwBreakpointBin` around the bottom bar hides the dots and keeps the sentence: the header already
says "Step 3 of 8", and one accurate statement beats a decoration drawn over it.

### Things in the model that look like over-engineering but are not

- **Every level has an `extra: Mapping` that swallows unknown keys, and they are written back on
  export.** The brief says an import must report what it could not understand rather than silently
  dropping it — `collect_notes` turns those leftovers into plain-English notes ("`cleanup` was kept
  exactly as written, but this app can't edit it yet"). Removing the flatten would quietly destroy
  users' manifests.
- **`sources:` and `modules:` entries can be a bare string, not just a mapping.** flatpak-builder
  treats a string as a path to a file holding more of them, and that is how *every* vendored
  dependency list (`generated-sources.json`) and every `shared-modules/…json` is wired in. A model
  that only understood mappings failed on the first real manifest tried against it — hence
  `SourceEntry`/`ModuleEntry`, and `main_module()` skipping trailing includes.
- **`runtime-version` is read leniently.** `runtime-version: 50` unquoted is a YAML number; refusing
  it would fail an import over something we can simply read. It is written back quoted.
- **`Deserialize` for the entry enums is hand-written rather than `#[serde(untagged)]`.** Untagged
  collapses every failure into "data did not match any variant", and explaining what actually went
  wrong is the point of the importer.
- **JSON export has a small hand-rolled writer** rather than `serde_json`: one more crate to
  transcribe into `generated-sources.json` for the offline build, for one function.

### Flatpak specifics

`flatpak/no.oyzmo.PackItFlat.yml` resolves its paths **relative to `flatpak/`, not the project
root** — both source entries climb out with `..`, and `makeflatpak.sh` preflights exactly that,
because `path: .` there hands the builder the wrong directory and the build dies on a missing
`Cargo.toml`.

**There is a second manifest, and nothing here builds it.** `flatpak/flathub/no.oyzmo.PackItFlat.yml`
is the copy for a Flathub submission: Flathub builds only from a published address, so a `dir` source
is not accepted and that copy fetches a git tag instead, with `generated-sources.json` committed
beside it in the Flathub repository. Everything else — permissions, build commands, runtime — must
stay identical, and `tests/flathub_manifest.rs` fails if it doesn't; a copy nothing builds is a copy
that drifts, and a permission missing from it is a round trip with a reviewer while an install line
missing from it is an app that installs without its icon. `bump.sh` moves its `tag:` with every
version for the same reason. The `commit:` is left as zeros on purpose: only the person making the
tag knows it, and a tag alone is not a repeatable build.

The two permissions a reviewer will ask about are `--filesystem=home` and
`--talk-name=org.freedesktop.Flatpak`, both justified in the metainfo. Checked against what Flathub
actually ships today: `io.github.flattool.Warehouse` has `org.freedesktop.Flatpak=talk`, and
`org.gnome.Builder` has that plus `filesystems=host`. The answer worth giving is that the app is
fully usable without the host connection — every file is still written, and only the optional build
button turns itself off.

`generated-sources.json` (87 crates) is generated by `flatpak/cargo-sources.py`, a stdlib-only
transcription of `Cargo.lock`, not by upstream's `flatpak-cargo-generator.py` (which needs
`aiohttp`/`toml`, neither packaged here, purely to resolve git dependencies — this app has none).
The script exits with a pointer to upstream if a git dependency ever appears; don't hand-edit the
JSON around it.

`build.rs` compiles `data/packitflat.gresource.xml` with `glib-compile-resources` directly rather
than through the `glib-build-tools` crate — that crate only shells out to the same binary and would
be one more entry to vendor. The icon is laid out in the bundle as an icon-theme directory
(`/no/oyzmo/PackItFlat/icons/scalable/apps/…`) so it resolves by name from the source tree and from
the Flatpak with no install step.

App ID, `.desktop` name, metainfo name and icon name all match. The app enforces that rule for its
users; it follows it itself.

### The crate list is checked against an independent implementation

`tests/vendor_crosscheck.rs` runs `vendor::cargo_sources` and `flatpak/cargo-sources.py` over this
project's own `Cargo.lock` and compares the results entry by entry. Two implementations — one in
Rust reading the lock file line by line, one in Python using `tomllib` — agreeing on 87 crates'
URLs, checksums and destinations is the evidence that the Rust one is right. Keep that test if you
touch either.

### Generated files are checked against the real tools

`tests/wizard_flow.rs` writes a full set of files and then runs `desktop-file-validate` and
`appstreamcli validate --no-net` over them, skipping either check where the tool isn't installed.
The desktop entry and the metainfo are written by hand — no library — so the only honest way to know
they are right is to hand them to the programs that judge them.
