//! The Flatpak manifest as a typed model, plus YAML/JSON import and export.
//!
//! Two rules shape everything here:
//!
//! 1. **Nothing the user wrote is ever dropped.** Every level of the model has
//!    an `extra` map that swallows keys this app doesn't model, and they are
//!    written back out on export. What lands there is *reported*, not silently
//!    kept, so the import summary can say "I kept `cleanup`, but you'll have to
//!    edit it by hand."
//! 2. **No error reaches the UI raw.** Parse failures come back as
//!    [`ManifestError`], which carries a plain sentence and, for YAML syntax
//!    errors, the line and column to jump to.
//!
//! JSON is valid YAML, so [`parse_str`] imports both without a second code path.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_yaml_ng::{Mapping, Value};
use thiserror::Error;

/// Runtime the wizard suggests when it has nothing better to go on.
pub const DEFAULT_RUNTIME: &str = "org.gnome.Platform";
pub const DEFAULT_RUNTIME_VERSION: &str = "50";
pub const DEFAULT_SDK: &str = "org.gnome.Sdk";

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("{message}")]
    Syntax {
        message: String,
        /// 1-based, when the parser could pin it down.
        line: Option<usize>,
        column: Option<usize>,
    },
    #[error("{0}")]
    Shape(String),
}

impl ManifestError {
    fn syntax(err: serde_yaml_ng::Error) -> Self {
        let location = err.location();
        // serde_yaml's own text is developer-facing ("mapping values are not
        // allowed in this context"); keep it, but wrap it in a sentence that
        // says what to do about it.
        ManifestError::Syntax {
            message: format!("This file could not be read as YAML: {err}"),
            line: location.as_ref().map(|l| l.line()),
            column: location.as_ref().map(|l| l.column()),
        }
    }

    /// One line, safe to put straight in front of a beginner.
    pub fn friendly(&self) -> String {
        match self {
            ManifestError::Syntax {
                message,
                line: Some(line),
                column: Some(column),
            } => format!("{message} (line {line}, column {column})"),
            ManifestError::Syntax { message, .. } => message.clone(),
            ManifestError::Shape(message) => message.clone(),
        }
    }
}

/// How serious an import note is. The UI colours them; nothing here is fatal —
/// a fatal problem is a [`ManifestError`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteLevel {
    /// Understood and imported, but worth mentioning.
    Info,
    /// Kept verbatim but not editable in the app, or missing and defaulted.
    Warning,
}

#[derive(Debug, Clone)]
pub struct ImportNote {
    pub level: NoteLevel,
    /// Where in the manifest, in the user's terms: "modules → packitflat".
    pub where_: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    pub notes: Vec<ImportNote>,
}

impl ImportReport {
    fn note(&mut self, level: NoteLevel, where_: impl Into<String>, message: impl Into<String>) {
        self.notes.push(ImportNote {
            level,
            where_: where_.into(),
            message: message.into(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn warnings(&self) -> usize {
        self.notes
            .iter()
            .filter(|n| n.level == NoteLevel::Warning)
            .count()
    }
}

/// The result of importing someone else's manifest.
#[derive(Debug, Clone)]
pub struct Import {
    pub manifest: Manifest,
    pub report: ImportReport,
}

/// Build system of a module. `Other` keeps anything flatpak-builder gains
/// later working instead of failing the import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum BuildSystem {
    Simple,
    Meson,
    CMake,
    CMakeNinja,
    Autotools,
    QMake,
    Other(String),
}

impl From<String> for BuildSystem {
    fn from(s: String) -> Self {
        match s.as_str() {
            "simple" => BuildSystem::Simple,
            "meson" => BuildSystem::Meson,
            "cmake" => BuildSystem::CMake,
            "cmake-ninja" => BuildSystem::CMakeNinja,
            "autotools" => BuildSystem::Autotools,
            "qmake" => BuildSystem::QMake,
            _ => BuildSystem::Other(s),
        }
    }
}

impl From<BuildSystem> for String {
    fn from(b: BuildSystem) -> Self {
        b.to_string()
    }
}

impl fmt::Display for BuildSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BuildSystem::Simple => "simple",
            BuildSystem::Meson => "meson",
            BuildSystem::CMake => "cmake",
            BuildSystem::CMakeNinja => "cmake-ninja",
            BuildSystem::Autotools => "autotools",
            BuildSystem::QMake => "qmake",
            BuildSystem::Other(s) => s,
        };
        f.write_str(s)
    }
}

/// Where a module's code comes from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum SourceKind {
    #[default]
    Dir,
    Git,
    Archive,
    File,
    Patch,
    Inline,
    Script,
    Shell,
    ExtraData,
    Other(String),
}

impl From<String> for SourceKind {
    fn from(s: String) -> Self {
        match s.as_str() {
            "dir" => SourceKind::Dir,
            "git" => SourceKind::Git,
            "archive" => SourceKind::Archive,
            "file" => SourceKind::File,
            "patch" => SourceKind::Patch,
            "inline" => SourceKind::Inline,
            "script" => SourceKind::Script,
            "shell" => SourceKind::Shell,
            "extra-data" => SourceKind::ExtraData,
            _ => SourceKind::Other(s),
        }
    }
}

impl From<SourceKind> for String {
    fn from(k: SourceKind) -> Self {
        k.to_string()
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SourceKind::Dir => "dir",
            SourceKind::Git => "git",
            SourceKind::Archive => "archive",
            SourceKind::File => "file",
            SourceKind::Patch => "patch",
            SourceKind::Inline => "inline",
            SourceKind::Script => "script",
            SourceKind::Shell => "shell",
            SourceKind::ExtraData => "extra-data",
            SourceKind::Other(s) => s,
        };
        f.write_str(s)
    }
}

impl SourceKind {
    /// One line of plain English for the UI. No jargon, no flatpak vocabulary.
    pub fn explanation(&self) -> &'static str {
        match self {
            SourceKind::Dir => "A folder on this computer",
            SourceKind::Git => "Code checked out from a Git repository",
            SourceKind::Archive => "A .tar or .zip downloaded from the internet",
            SourceKind::File => "A single file copied into the build",
            SourceKind::Patch => "A patch applied to the code before building",
            SourceKind::Inline => "A small file written out from text in the manifest",
            SourceKind::Script => "A script written out from lines in the manifest",
            SourceKind::Shell => "Shell commands run before the build starts",
            SourceKind::ExtraData => "A file downloaded when the app is installed",
            SourceKind::Other(_) => "A source type this app doesn't know about",
        }
    }
}

/// One `sources:` entry. Flat rather than an enum per type: the fields overlap
/// heavily, and a flat struct round-trips unknown keys through `extra` without
/// a hand-written deserializer per variant.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: SourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest_filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contents: Option<String>,
    #[serde(flatten)]
    pub extra: Mapping,
}

impl Source {
    /// A local folder source, the default for "point at my project".
    pub fn dir(path: impl Into<String>) -> Self {
        Source {
            kind: SourceKind::Dir,
            path: Some(path.into()),
            ..Source::default()
        }
    }

    /// What to show in a list: the useful half of the entry, not the type name.
    pub fn label(&self) -> String {
        match (&self.path, &self.url) {
            (Some(path), _) => path.clone(),
            (_, Some(url)) => url.clone(),
            _ => self.kind.to_string(),
        }
    }
}

/// A `sources:` entry. flatpak-builder accepts either the full mapping or a
/// bare string naming a file that holds more sources — that shorthand is how
/// every vendored-dependency list (`generated-sources.json`) is wired in, so an
/// importer that only understands mappings chokes on the first real manifest it
/// meets.
// The big variant is the common one — an include is the rare shorthand — so
// boxing it would put every ordinary source on the heap to save nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum SourceEntry {
    Source(Source),
    /// A path to another file listing sources.
    Include(String),
}

impl SourceEntry {
    pub fn label(&self) -> String {
        match self {
            SourceEntry::Source(source) => source.label(),
            SourceEntry::Include(path) => path.clone(),
        }
    }

    pub fn as_source(&self) -> Option<&Source> {
        match self {
            SourceEntry::Source(source) => Some(source),
            SourceEntry::Include(_) => None,
        }
    }
}

impl Serialize for SourceEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            SourceEntry::Source(source) => source.serialize(s),
            SourceEntry::Include(path) => s.serialize_str(path),
        }
    }
}

impl<'de> Deserialize<'de> for SourceEntry {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        // Hand-written rather than `#[serde(untagged)]`: untagged collapses any
        // failure into "data did not match any variant", and the whole point of
        // the importer is to say what actually went wrong.
        match Value::deserialize(de)? {
            Value::String(path) => Ok(SourceEntry::Include(path)),
            other => Source::deserialize(other)
                .map(SourceEntry::Source)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// `build-options:`, the settings that apply while compiling rather than to the
/// finished app. Modelled rather than left in `extra` because the wizard edits
/// the environment: a Rust build needs `CARGO_HOME` pointed inside the build
/// directory or it tries to reach the network, which a Flatpak build cannot do.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BuildOptions {
    #[serde(default, skip_serializing_if = "Mapping::is_empty")]
    pub env: Mapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepend_ld_library_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_args: Vec<String>,
    #[serde(flatten)]
    pub extra: Mapping,
}

impl BuildOptions {
    pub fn is_empty(&self) -> bool {
        self == &BuildOptions::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Module {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buildsystem: Option<BuildSystem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_options: Option<BuildOptions>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_opts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceEntry>,
    #[serde(flatten)]
    pub extra: Mapping,
}

impl Module {
    pub fn new(name: impl Into<String>) -> Self {
        Module {
            name: name.into(),
            ..Module::default()
        }
    }
}

/// A `modules:` entry. Same shorthand as sources — `shared-modules/…json` in
/// someone else's manifest is a string, not a mapping.
// As with SourceEntry: the big variant is the ordinary one, so boxing it would
// cost an allocation per module to shrink the rare shorthand.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ModuleEntry {
    Module(Module),
    Include(String),
}

impl ModuleEntry {
    pub fn name(&self) -> String {
        match self {
            ModuleEntry::Module(module) => module.name.clone(),
            ModuleEntry::Include(path) => path.clone(),
        }
    }

    pub fn as_module(&self) -> Option<&Module> {
        match self {
            ModuleEntry::Module(module) => Some(module),
            ModuleEntry::Include(_) => None,
        }
    }
}

impl Serialize for ModuleEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            ModuleEntry::Module(module) => module.serialize(s),
            ModuleEntry::Include(path) => s.serialize_str(path),
        }
    }
}

impl<'de> Deserialize<'de> for ModuleEntry {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        match Value::deserialize(de)? {
            Value::String(path) => Ok(ModuleEntry::Include(path)),
            other => Module::deserialize(other)
                .map(ModuleEntry::Module)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub runtime: String,
    /// Quoted in every well-formed manifest, but `runtime-version: 50` parses
    /// as a number and must not blow up the import.
    #[serde(default, deserialize_with = "lenient_string")]
    pub runtime_version: String,
    #[serde(default)]
    pub sdk: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sdk_extensions: Vec<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub finish_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_options: Option<BuildOptions>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<ModuleEntry>,
    #[serde(flatten)]
    pub extra: Mapping,
}

/// Accepts a string, a number or a bool where a string is wanted. YAML turns
/// `runtime-version: 50` into a number and `branch: no` into `false`; refusing
/// those would fail the import over something we can simply read.
fn lenient_string<'de, D>(de: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(de)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Null => Ok(String::new()),
        other => Err(serde::de::Error::custom(format!(
            "expected a version like '50', found {other:?}"
        ))),
    }
}

impl Manifest {
    /// A manifest that already builds, for a new project. Every field the
    /// wizard would otherwise ask for has a defensible answer here, so pressing
    /// Next through the whole thing produces something that works.
    pub fn starter(app_id: &str, command: &str) -> Self {
        Manifest {
            app_id: app_id.to_string(),
            runtime: DEFAULT_RUNTIME.to_string(),
            runtime_version: DEFAULT_RUNTIME_VERSION.to_string(),
            sdk: DEFAULT_SDK.to_string(),
            sdk_extensions: Vec::new(),
            command: command.to_string(),
            // The four nearly every graphical app needs, and nothing else:
            // a permission the app doesn't ask for is one nobody has to trust.
            finish_args: vec![
                "--socket=wayland".into(),
                "--socket=fallback-x11".into(),
                "--share=ipc".into(),
                "--device=dri".into(),
            ],
            build_options: None,
            modules: Vec::new(),
            extra: Mapping::new(),
        }
    }

    pub fn to_yaml(&self) -> Result<String, ManifestError> {
        serde_yaml_ng::to_string(self).map_err(|e| {
            ManifestError::Shape(format!("This project could not be written as YAML: {e}"))
        })
    }

    /// JSON export, offered alongside YAML because some projects use it.
    pub fn to_json(&self) -> Result<String, ManifestError> {
        let value: Value = serde_yaml_ng::to_value(self).map_err(|e| {
            ManifestError::Shape(format!("This project could not be converted: {e}"))
        })?;
        json::to_string(&value)
    }

    /// The module that builds the app itself: by convention the last one, and
    /// the last *spelled-out* one — a trailing `shared-modules/…` include is a
    /// dependency, never the app.
    pub fn main_module(&self) -> Option<&Module> {
        self.modules.iter().rev().find_map(ModuleEntry::as_module)
    }

    pub fn main_module_mut(&mut self) -> Option<&mut Module> {
        self.modules
            .iter_mut()
            .rev()
            .find_map(|entry| match entry {
                ModuleEntry::Module(module) => Some(module),
                ModuleEntry::Include(_) => None,
            })
    }

    /// The main module, created if the manifest hasn't got one. The wizard edits
    /// through this, so an imported manifest that is nothing but includes still
    /// has somewhere to put the answers.
    pub fn ensure_main_module(&mut self, name: &str) -> &mut Module {
        if self.main_module().is_none() {
            self.modules.push(ModuleEntry::Module(Module::new(name)));
        }
        self.main_module_mut().expect("just pushed one")
    }
}

/// Parse a manifest that someone else wrote. Accepts YAML or JSON.
pub fn parse_str(text: &str) -> Result<Import, ManifestError> {
    let value: Value = serde_yaml_ng::from_str(text).map_err(ManifestError::syntax)?;
    let mut report = ImportReport::default();

    let Value::Mapping(mut map) = value else {
        return Err(ManifestError::Shape(
            "This file doesn't look like a Flatpak manifest — a manifest is a \
             list of settings such as app-id and runtime."
                .into(),
        ));
    };

    // `id:` is the older spelling of `app-id:` and still valid.
    if !map.contains_key(Value::from("app-id")) {
        if let Some(id) = map.remove(Value::from("id")) {
            map.insert(Value::from("app-id"), id);
            report.note(
                NoteLevel::Info,
                "app-id",
                "This manifest used the older name “id”. It means the same as \
                 “app-id” and has been read as one.",
            );
        }
    }

    let manifest: Manifest =
        serde_yaml_ng::from_value(Value::Mapping(map)).map_err(ManifestError::syntax)?;

    collect_notes(&manifest, &mut report);
    Ok(Import { manifest, report })
}

/// Everything the import couldn't act on, in the user's words. Unknown keys are
/// kept in the model and written back — the note exists so the user is told
/// they will have to edit those by hand rather than discovering it later.
fn collect_notes(manifest: &Manifest, report: &mut ImportReport) {
    if manifest.app_id.is_empty() {
        report.note(
            NoteLevel::Warning,
            "app-id",
            "No app ID was found. Every Flatpak needs one — you'll be asked for it.",
        );
    }
    if manifest.runtime.is_empty() {
        report.note(
            NoteLevel::Warning,
            "runtime",
            "No runtime was named, so the usual GNOME one has been suggested.",
        );
    }
    if manifest.command.is_empty() {
        report.note(
            NoteLevel::Warning,
            "command",
            "This manifest doesn't say which program to start when the app is launched.",
        );
    }

    report_extra(&manifest.extra, "The manifest itself", report);

    for entry in &manifest.modules {
        let module = match entry {
            ModuleEntry::Module(module) => module,
            ModuleEntry::Include(path) => {
                report.note(
                    NoteLevel::Warning,
                    "modules",
                    format!(
                        "This manifest pulls in another module file, “{path}”. That file \
                         hasn't been read — it stays exactly as it is, and whatever it \
                         builds keeps working."
                    ),
                );
                continue;
            }
        };

        let where_ = format!("module “{}”", module.name);
        report_extra(&module.extra, &where_, report);

        for entry in &module.sources {
            let source = match entry {
                SourceEntry::Source(source) => source,
                SourceEntry::Include(path) => {
                    report.note(
                        NoteLevel::Info,
                        &where_,
                        format!(
                            "Some of this module's code is listed in a separate file, \
                             “{path}” — usually the list of downloaded dependencies \
                             prepared for building without internet access. It has been \
                             left alone."
                        ),
                    );
                    continue;
                }
            };

            if let SourceKind::Other(kind) = &source.kind {
                report.note(
                    NoteLevel::Warning,
                    &where_,
                    format!(
                        "The source type “{kind}” isn't one this app understands. \
                         It has been kept exactly as written."
                    ),
                );
            }
            report_extra(
                &source.extra,
                &format!("{where_}, source “{}”", source.label()),
                report,
            );
        }
    }
}

fn report_extra(extra: &Mapping, where_: &str, report: &mut ImportReport) {
    let mut names: Vec<String> = extra
        .keys()
        .filter_map(|k| k.as_str().map(str::to_string))
        .collect();
    if names.is_empty() {
        return;
    }
    names.sort();
    let list = names
        .iter()
        .map(|n| format!("“{n}”"))
        .collect::<Vec<_>>()
        .join(", ");
    report.note(
        NoteLevel::Warning,
        where_,
        format!(
            "{list} {} kept exactly as written, but this app can't edit {} yet.",
            if names.len() == 1 { "was" } else { "were" },
            if names.len() == 1 { "it" } else { "them" }
        ),
    );
}

/// Minimal JSON writer. Export to JSON is a convenience, and serde_json would
/// be one more crate to vendor for the offline Flatpak build for the sake of
/// this one function.
mod json {
    use super::{ManifestError, Value};
    use std::fmt::Write;

    pub fn to_string(value: &Value) -> Result<String, ManifestError> {
        let mut out = String::new();
        write_value(value, 0, &mut out)?;
        out.push('\n');
        Ok(out)
    }

    fn write_value(value: &Value, indent: usize, out: &mut String) -> Result<(), ManifestError> {
        let pad = "  ".repeat(indent);
        let inner_pad = "  ".repeat(indent + 1);
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => {
                let _ = write!(out, "{b}");
            }
            Value::Number(n) => {
                let _ = write!(out, "{n}");
            }
            Value::String(s) => out.push_str(&quote(s)),
            Value::Sequence(items) if items.is_empty() => out.push_str("[]"),
            Value::Sequence(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&inner_pad);
                    write_value(item, indent + 1, out)?;
                    out.push_str(if i + 1 == items.len() { "\n" } else { ",\n" });
                }
                out.push_str(&pad);
                out.push(']');
            }
            Value::Mapping(map) if map.is_empty() => out.push_str("{}"),
            Value::Mapping(map) => {
                out.push_str("{\n");
                let len = map.len();
                for (i, (key, val)) in map.iter().enumerate() {
                    let key = key.as_str().ok_or_else(|| {
                        ManifestError::Shape(
                            "This project has a setting whose name isn't text, \
                             which JSON can't represent."
                                .into(),
                        )
                    })?;
                    out.push_str(&inner_pad);
                    out.push_str(&quote(key));
                    out.push_str(": ");
                    write_value(val, indent + 1, out)?;
                    out.push_str(if i + 1 == len { "\n" } else { ",\n" });
                }
                out.push_str(&pad);
                out.push('}');
            }
            Value::Tagged(tagged) => write_value(&tagged.value, indent, out)?,
        }
        Ok(())
    }

    fn quote(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = r#"
app-id: no.oyzmo.RmMeta
runtime: org.gnome.Platform
runtime-version: '50'
sdk: org.gnome.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: rmmeta
finish-args:
  - --socket=wayland
  - --device=dri
cleanup:
  - /include
modules:
  - name: rmmeta
    buildsystem: simple
    build-commands:
      - cargo --offline build --release
    sources:
      - type: dir
        path: ..
        skip:
          - dev
      - ../generated-sources.json
"#;

    fn main_module(m: &Manifest) -> &Module {
        m.main_module().expect("a spelled-out module")
    }

    #[test]
    fn reads_a_real_manifest() {
        let import = parse_str(REAL).expect("parses");
        let m = &import.manifest;
        assert_eq!(m.app_id, "no.oyzmo.RmMeta");
        assert_eq!(m.runtime_version, "50");
        assert_eq!(m.sdk_extensions.len(), 1);
        assert_eq!(m.modules.len(), 1);

        let module = main_module(m);
        assert_eq!(module.buildsystem, Some(BuildSystem::Simple));
        assert_eq!(
            module.sources[0].as_source().unwrap().path.as_deref(),
            Some("..")
        );
        // The shorthand: a bare string in `sources:` is a file listing more
        // sources, which is how every vendored dependency list is wired in.
        assert_eq!(
            module.sources[1],
            SourceEntry::Include("../generated-sources.json".into())
        );
    }

    #[test]
    fn module_and_source_includes_survive_a_round_trip() {
        let text = "app-id: a.b.C\nmodules:\n  - shared-modules/glew/glew.json\n  - name: app\n";
        let import = parse_str(text).unwrap();
        assert_eq!(
            import.manifest.modules[0],
            ModuleEntry::Include("shared-modules/glew/glew.json".into())
        );
        // The app is the last spelled-out module, not the include.
        assert_eq!(main_module(&import.manifest).name, "app");

        let out = import.manifest.to_yaml().unwrap();
        assert!(out.contains("- shared-modules/glew/glew.json"));
        assert_eq!(parse_str(&out).unwrap().manifest, import.manifest);

        // and the user is told the file wasn't read
        assert!(import
            .report
            .notes
            .iter()
            .any(|n| n.message.contains("shared-modules/glew/glew.json")));
    }

    #[test]
    fn unknown_keys_are_kept_and_reported() {
        let import = parse_str(REAL).unwrap();
        // `cleanup` at the top level, `skip` inside a source.
        assert!(import.manifest.extra.contains_key(Value::from("cleanup")));
        assert!(main_module(&import.manifest).sources[0]
            .as_source()
            .unwrap()
            .extra
            .contains_key(Value::from("skip")));

        let out = import.manifest.to_yaml().unwrap();
        assert!(out.contains("cleanup"), "unknown keys survive export");
        assert!(out.contains("skip"));

        let mentioned: Vec<&str> = import.report.notes.iter().map(|n| n.where_.as_str()).collect();
        assert!(mentioned.contains(&"The manifest itself"));
        assert_eq!(import.report.warnings(), 2);
    }

    #[test]
    fn round_trips_without_losing_anything() {
        let once = parse_str(REAL).unwrap().manifest;
        let text = once.to_yaml().unwrap();
        let twice = parse_str(&text).unwrap().manifest;
        assert_eq!(once, twice);
    }

    #[test]
    fn unquoted_runtime_version_is_read_as_text() {
        let import = parse_str("app-id: a.b.C\nruntime-version: 50\n").unwrap();
        assert_eq!(import.manifest.runtime_version, "50");
        // and comes back out quoted, the way flatpak-builder wants it
        assert!(import.manifest.to_yaml().unwrap().contains("'50'"));
    }

    #[test]
    fn legacy_id_key_is_understood() {
        let import = parse_str("id: no.oyzmo.Old\nruntime: org.gnome.Platform\n").unwrap();
        assert_eq!(import.manifest.app_id, "no.oyzmo.Old");
        assert!(import.report.notes.iter().any(|n| n.where_ == "app-id"));
    }

    #[test]
    fn json_manifests_import_too() {
        let import = parse_str(r#"{"app-id": "no.oyzmo.J", "modules": [{"name": "j"}]}"#).unwrap();
        assert_eq!(import.manifest.app_id, "no.oyzmo.J");
        assert_eq!(main_module(&import.manifest).name, "j");
    }

    #[test]
    fn json_export_is_parseable_again() {
        let manifest = parse_str(REAL).unwrap().manifest;
        let json = manifest.to_json().unwrap();
        assert!(json.starts_with('{'));
        let back = parse_str(&json).unwrap().manifest;
        assert_eq!(manifest, back);
    }

    #[test]
    fn syntax_errors_carry_a_place_to_look() {
        let err = parse_str("app-id: fine\n  bad: indent\n").unwrap_err();
        match err {
            ManifestError::Syntax { line, .. } => assert!(line.is_some()),
            other => panic!("expected a syntax error, got {other:?}"),
        }
        // and never panics on arbitrary input
        assert!(parse_str("- just\n- a list\n").is_err());
        assert!(parse_str("").is_err() || parse_str("").is_ok());
    }

    #[test]
    fn missing_essentials_are_reported_not_fatal() {
        let import = parse_str("modules: []\n").unwrap();
        assert_eq!(import.report.warnings(), 3); // app-id, runtime, command
    }

    #[test]
    fn starter_manifest_has_working_defaults() {
        let m = Manifest::starter("no.oyzmo.PackItFlat", "packitflat");
        assert_eq!(m.runtime, DEFAULT_RUNTIME);
        assert!(m.finish_args.iter().any(|a| a == "--socket=wayland"));
        assert!(!m.finish_args.iter().any(|a| a.contains("filesystem=host")));
        assert!(m.to_yaml().unwrap().contains("app-id: no.oyzmo.PackItFlat"));
    }
}
