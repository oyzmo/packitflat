````markdown
# Project Brief: Pack It Flat

A GNOME app that makes creating a Flatpak approachable for people who have
never written a manifest. Guided, step-by-step, in plain language.

## Core stack & constraints

- Rust, GTK4 + Libadwaita (latest stable Adw). Use `.ui` files or Blueprint —
  do NOT construct widgets by hand in Rust; keep `imp` blocks small.
- Manifest format: **YAML** (JSON only as an export/import option).
- Async via `gio::Subprocess` + `glib::spawn_future_local`. No tokio, never
  block the main loop.
- Errors: `anyhow` internally, `thiserror` for domain errors. No `unwrap()`
  or `expect()` on any path reachable from the UI.
- The app itself ships as a Flatpak (dogfood it). Meson build system.
- App ID suggestion: `io.github.<user>.FlatpakStudio` — make it easy to change.

## Sandbox / host access (decide this early, it shapes everything)

The app generates files AND can optionally run the build. Since it is itself
sandboxed:

- Run `flatpak-builder` on the host via `flatpak-spawn --host`.
- Manifest needs `--talk-name=org.freedesktop.Flatpak` and
  `--filesystem=home` (justify this in the metainfo; do not use
  `--filesystem=host`).
- Detect at startup whether `flatpak-spawn` works. If not, disable the build
  button with a clear explanation and a copyable command the user can paste
  into a terminal instead.
- Everything except running the build must work with zero host access.

## Guiding principle: assume the user knows nothing

This is the most important requirement. Concretely:

- **Every input has three things**: a short label, one line of plain-English
  explanation under it, and a realistic placeholder example.
- **Nothing is a bare technical term.** "finish-args" is never shown as
  "finish-args" — it's "Permissions". `--share=network` is "Let the app use
  the internet".
- An `AdwExpanderRow` "What is this?" on each step with 2–4 sentences and a
  link to the relevant docs.
- **Sensible defaults everywhere** — a user should be able to press Next
  through the whole wizard on a detected project and get a working build.
- **Auto-detect the project type** by scanning the source folder:
  `Cargo.toml` → Rust/cargo, `meson.build` → Meson, `CMakeLists.txt` → CMake,
  `package.json` → Node, `pyproject.toml`/`setup.py` → Python, `configure`/
  `Makefile` → autotools/simple. Pre-fill build-system, build-args and
  likely runtime from that, and *tell the user what was detected and why*.
- **Never surface a raw error.** Map known flatpak-builder failures to a
  human sentence + a suggested fix button. Minimum set to handle:
  - runtime/SDK not installed → "Install it" button
  - network access during build → explain offline builds, offer to generate
    vendored sources
  - sha256 mismatch → offer to recompute
  - app ID / desktop / metainfo filename mismatch → offer to rename
  - missing flathub remote → "Add Flathub" button
  - unknown → show the raw log in an expander, with a "Copy log" button.
- A **Glossary** page in the main menu (runtime, SDK, sandbox, portal,
  manifest, module, remote, bundle) written for beginners.
- **First-run check**: is `flatpak-builder` installed, is the Flathub remote
  added, is any runtime present? Show a checklist with fix buttons.

## New project flow

On "New Project", ask (in an `AdwAlertDialog` or a small setup page):

1. Point at a source folder, or start from a template.
2. "How do you want to work?" → **Guided steps (recommended)** / **Editor**.
   Guided is preselected. A checkbox "Remember my choice" writes the default
   to GSettings; changeable later in Preferences.

Both modes edit the same in-memory model and can be switched at any time from
a view switcher in the header bar. Never lose data when switching.

Also offer **Import existing manifest** (YAML or JSON) from the welcome page —
parse it into the model, report anything it couldn't understand rather than
silently dropping it.

## Wizard steps (`AdwNavigationView`)

Steps are re-enterable; a sidebar or carousel dots show progress and which
steps still have problems. Each step validates inline (green check / amber
warning / red error) and a persistent bottom bar shows "2 issues left" with a
jump-to link.

1. **Basics** — app name, app ID (validate reverse-DNS + D-Bus rules, no
   hyphens in the last segment, explain the rule when it fails), summary,
   description, license (SPDX picker with search), homepage, developer name.
2. **Runtime & SDK** — populate from `flatpak remote-ls flathub --runtime`
   (cache it; fall back to a bundled list offline). Show friendly names
   ("GNOME 48"), warn on EOL runtimes, offer one-click install of missing
   SDK/runtime with a progress row. Include SDK extensions (rust-stable,
   node, openjdk…) with checkboxes.
3. **Sources** — local dir, git (url + tag/commit/branch), archive (url +
   sha256, with a "Compute hash" button that downloads and hashes with a
   progress bar), or file/patch. Multiple sources per module.
4. **Build system** — pre-filled from detection. Fields for config-opts,
   build-args, env, make-args. Show the resulting module YAML live in a
   collapsible preview.
5. **Dependencies / offline builds** — explain in one paragraph that builds
   have no network. If Rust/Node/Python detected, offer to run the
   appropriate generator (`flatpak-cargo-generator.py`,
   `flatpak-node-generator`, `req2flatpak`) and wire the resulting
   `cargo-sources.json` etc. into the manifest automatically. Allow adding
   extra modules (shared libs) manually, and provide a small library of
   common prebuilt module snippets.
6. **Permissions** — the safety-critical step. Grouped, plain-language
   toggles: Display (wayland/x11/fallback-x11), Audio (pulseaudio), Network,
   GPU (dri), Files (home / documents / specific paths / none), Devices,
   D-Bus names, Environment. Each toggle explains what breaks if it's off.
   Show a live **risk summary** ("This app can read every file in your home
   folder"), flag `--filesystem=host`, `--share=network` and wildcard
   `--talk-name` with amber warnings, and suggest the portal alternative
   ("Use the file chooser portal instead — no permission needed").
7. **Appearance & metadata** — icon picker (accept SVG/PNG, auto-generate
   the hicolor sizes and correct filenames), `.desktop` fields (categories
   picker, keywords, StartupWMClass), and the AppStream metainfo: OARS
   content rating (a short questionnaire, not raw OARS ids), screenshots,
   release notes/version. Explain that Flathub requires these.
8. **Review & generate** — a file-by-file list of everything about to be
   written with a one-line "what this file does" and a diff/preview for each.
   Checkboxes to skip individual files. Then **Generate**, and afterwards a
   "Build now" button (optional, never automatic).

## Editor mode

- Left: tree of modules/sources/permissions. Right: form panes.
- A **raw YAML view** with syntax highlighting (GtkSourceView) that is
  two-way synced with the model. If the user's hand-edits can't be parsed,
  show the error with line/column and don't clobber their text.
- Same validation panel as the wizard.

## Generated output

Written into the project folder (ask before overwriting; offer a backup):

- `<app-id>.yml` — the manifest
- `<app-id>.desktop`
- `<app-id>.metainfo.xml` (AppStream, with OARS rating and SPDX license)
- `icons/hicolor/…/apps/<app-id>.(svg|png)`
- `cargo-sources.json` / `node-sources.json` etc. when applicable
- `.gitignore` covering `.flatpak-builder/`, `build/`, `repo/`, `*.flatpak`
- Optionally a `build.sh` with the exact commands, so the user can see and
  reuse them outside the app.

Enforce that app ID, desktop filename, metainfo filename and icon filename
all match — this is the single most common beginner mistake.

## Build runner

- Preflight: flatpak-builder present, runtime + SDK installed, manifest
  parses, `flatpak-builder --show-deps` dry run. Show results as a checklist.
- Run `flatpak-builder --force-clean --user --install-deps-from=flathub
  build <manifest>` (options exposed as toggles, with explanations).
- Streaming, scrollable, searchable log with a Cancel button. Show a
  friendly status line above it ("Compiling — this can take several
  minutes") rather than making the log the primary UI.
- On success, offer: **Install** (`--user --install`), **Run**, **Export
  bundle** (`flatpak build-bundle`), **Open build folder**.
- On failure, run the error translator described above.

## Other requirements

- **Autosave project state** to `~/.local/share/flatpak-studio/projects/` so a
  half-finished setup survives a crash or restart. Recent projects on the
  welcome page.
- Templates/presets: GTK4 + Rust, GTK4 + Python, GTK4 + C/Meson, Electron/
  Node, generic autotools, "just wrap a binary".
- Full keyboard nav, screen-reader labels, and it must work at 360px width
  (mobile/adaptive) — use `AdwBreakpoint`.
- Translations: gettext from day one; don't hardcode user-visible strings.
- Dark mode via Adwaita defaults, no custom CSS beyond small accents.
- Ship a `HACKING.md` explaining the module layout.

## Suggested milestones

1. App skeleton, welcome page, project model + YAML (de)serialization,
   import existing manifest.
2. Wizard steps 1–4 + generate manifest only.
3. Validation framework + editor mode + raw YAML sync.
4. Permissions step with risk summary; desktop/metainfo/icon generation.
5. Build runner with streaming log, preflight and error translation.
6. Offline dependency generators, templates, post-build actions, polish.

## Non-goals (say no to these)

- No Flathub submission automation (PR creation) in v1.
- No arbitrary shell script editing UI.
- No support for non-Flatpak packaging formats.

## First questions to answer before writing code

Ask me if anything here is ambiguous, and propose a module/file layout for
review before implementing.
````