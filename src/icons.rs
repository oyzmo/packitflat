//! Putting the app's picture where Flatpak will find it.
//!
//! An icon that doesn't show up is one of the classic beginner failures, and it
//! is always the same two causes: the file is in the wrong folder, or it isn't
//! named after the app ID. Both are decided here, from the picture the user
//! picked, so neither can be typed wrongly.
//!
//! The file itself is examined rather than trusted: a `.png` that is really a
//! JPEG, or a 40×40 icon that a store will reject, is worth saying out loud
//! before the build rather than after it.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Sizes hicolor uses and stores expect. 128 is the smallest Flathub accepts.
pub const STANDARD_SIZES: &[u32] = &[512, 256, 128, 64, 48];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Drawn at any size — always the best answer.
    Svg,
    Png { width: u32, height: u32 },
}

#[derive(Debug, Error)]
pub enum IconError {
    #[error("{0} couldn't be read.")]
    Unreadable(String),
    #[error("{0} isn't a picture this app can use. It has to be an SVG or a PNG.")]
    Unsupported(String),
    #[error("{0}")]
    Failed(String),
}

impl IconError {
    pub fn friendly(&self) -> String {
        self.to_string()
    }
}

/// What kind of picture this is, by looking at the file rather than its name.
pub fn inspect(path: &Path) -> Result<Kind, IconError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let bytes =
        fs::read(path).map_err(|_| IconError::Unreadable(name.clone()))?;

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (width, height) =
            png_size(&bytes).ok_or_else(|| IconError::Unsupported(name.clone()))?;
        return Ok(Kind::Png { width, height });
    }

    // SVG is text, and may open with an XML declaration, a comment or a
    // doctype before the <svg> tag ever appears.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]);
    if head.contains("<svg") {
        return Ok(Kind::Svg);
    }

    Err(IconError::Unsupported(name))
}

/// Width and height out of the IHDR chunk, which a PNG always starts with.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

/// Where this picture belongs, relative to the project folder. Always
/// `icons/hicolor/<size>/apps/<app-id>.<ext>` — the layout flatpak-builder
/// installs from and the name the desktop entry's `Icon=` line points at.
pub fn target_path(app_id: &str, kind: &Kind) -> PathBuf {
    let dir = match kind {
        Kind::Svg => "scalable".to_string(),
        Kind::Png { width, height } => format!("{width}x{height}"),
    };
    let extension = match kind {
        Kind::Svg => "svg",
        Kind::Png { .. } => "png",
    };
    Path::new("icons")
        .join("hicolor")
        .join(dir)
        .join("apps")
        .join(format!("{app_id}.{extension}"))
}

/// Take up the icon a project already has, if it hasn't got one chosen.
///
/// Called wherever the answer could have changed, not only when the project is
/// opened: a project started from a folder has no app ID yet, and the icon is
/// named after it — so at open there is nothing to look for, and by the time the
/// ID is typed nobody would look again. Cheap enough to call on every edit.
pub fn adopt_existing(project: &mut crate::project::Project) {
    if project.icon_source.is_some() {
        return;
    }
    let Some(folder) = project.source_dir.clone() else {
        return;
    };
    project.icon_source = existing(&folder, &project.manifest.app_id);
}

/// The icon a project already has, if there is one where this app puts them.
///
/// A manifest opened on its own carries no memory of which picture was chosen —
/// the manifest has nowhere to record it — so a project that has been through
/// here before came back without an icon, and with it went the line that
/// installs it into the finished app. The file sitting at the conventional path
/// *is* the answer, so it is adopted rather than asked for again.
pub fn existing(project_dir: &Path, app_id: &str) -> Option<PathBuf> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let hicolor = project_dir.join("icons").join("hicolor");

    let scalable = hicolor
        .join("scalable")
        .join("apps")
        .join(format!("{app_id}.svg"));
    if scalable.is_file() {
        return Some(scalable);
    }

    // Otherwise the biggest PNG there is: it is the one that scales down best,
    // and AppStream wants at least 64 across.
    fs::read_dir(&hicolor)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let size: u32 = name.to_string_lossy().split('x').next()?.parse().ok()?;
            let path = entry.path().join("apps").join(format!("{app_id}.png"));
            path.is_file().then_some((size, path))
        })
        .max_by_key(|(size, _)| *size)
        .map(|(_, path)| path)
}

/// Anything worth saying about the chosen picture before it is used. Empty means
/// it is fine as it is.
pub fn advice(kind: &Kind) -> Vec<String> {
    match kind {
        Kind::Svg => Vec::new(),
        Kind::Png { width, height } if width != height => vec![format!(
            "This picture is {width}×{height}. App icons have to be square, or they \
             will be squashed."
        )],
        Kind::Png { width, .. } if *width < 128 => vec![format!(
            "This picture is {width} pixels across. Stores want at least 128, and an \
             SVG is better still — it stays sharp at every size."
        )],
        Kind::Png { width, .. } if !STANDARD_SIZES.contains(width) => vec![format!(
            "This picture is {width} pixels across, which isn't one of the sizes desktops \
             look in ({}). It will be installed anyway, but a standard size or an SVG is \
             safer.",
            STANDARD_SIZES
                .iter()
                .map(|size| size.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )],
        Kind::Png { .. } => vec![
            "A PNG works, but an SVG stays sharp on every screen and is what most \
             GNOME apps ship."
                .to_string(),
        ],
    }
}

#[derive(Debug, Clone)]
pub struct Installed {
    pub path: PathBuf,
    pub kind: Kind,
}

/// Copy the picture into the project, under the name and folder Flatpak expects.
/// The source is never moved: it is the user's file, and they may want it where
/// it is.
pub fn install(source: &Path, project_dir: &Path, app_id: &str) -> Result<Installed, IconError> {
    let kind = inspect(source)?;
    let relative = target_path(app_id, &kind);
    let target = project_dir.join(&relative);

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            IconError::Failed(format!("{} couldn't be created: {e}", parent.display()))
        })?;
    }

    // Same file, already in place: copying it onto itself would truncate it.
    if source
        .canonicalize()
        .ok()
        .zip(target.canonicalize().ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false)
    {
        return Ok(Installed {
            path: relative,
            kind,
        });
    }

    fs::copy(source, &target).map_err(|e| {
        IconError::Failed(format!("{} couldn't be written: {e}", target.display()))
    })?;

    Ok(Installed {
        path: relative,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 PNG, and the smallest real one there is.
    fn png_bytes(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::from(b"\x89PNG\r\n\x1a\n".as_slice());
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn the_file_is_judged_by_its_contents_not_its_name() {
        let dir = tempfile::tempdir().unwrap();

        let png = write(dir.path(), "icon.svg", &png_bytes(128, 128));
        assert_eq!(
            inspect(&png).unwrap(),
            Kind::Png {
                width: 128,
                height: 128
            }
        );

        let svg = write(
            dir.path(),
            "icon.png",
            b"<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        );
        assert_eq!(inspect(&svg).unwrap(), Kind::Svg);

        let junk = write(dir.path(), "notes.txt", b"just some words");
        assert!(matches!(
            inspect(&junk),
            Err(IconError::Unsupported(_))
        ));
        assert!(inspect(&dir.path().join("missing.png")).is_err());
    }

    #[test]
    fn everything_is_named_after_the_app_id() {
        assert_eq!(
            target_path("no.oyzmo.PackItFlat", &Kind::Svg),
            Path::new("icons/hicolor/scalable/apps/no.oyzmo.PackItFlat.svg")
        );
        assert_eq!(
            target_path(
                "no.oyzmo.PackItFlat",
                &Kind::Png {
                    width: 256,
                    height: 256
                }
            ),
            Path::new("icons/hicolor/256x256/apps/no.oyzmo.PackItFlat.png")
        );
    }

    #[test]
    fn an_svg_needs_no_advice() {
        assert!(advice(&Kind::Svg).is_empty());
    }

    #[test]
    fn pictures_that_will_disappoint_are_flagged_before_the_build() {
        let squashed = advice(&Kind::Png {
            width: 200,
            height: 100,
        });
        assert!(squashed[0].contains("square"));

        let tiny = advice(&Kind::Png {
            width: 48,
            height: 48,
        });
        assert!(tiny[0].contains("at least 128"));

        let odd = advice(&Kind::Png {
            width: 200,
            height: 200,
        });
        assert!(odd[0].contains("isn't one of the sizes"));

        let fine = advice(&Kind::Png {
            width: 256,
            height: 256,
        });
        assert_eq!(fine.len(), 1, "a standard PNG gets the SVG suggestion only");
        assert!(fine[0].contains("SVG"));
    }

    /// A project started from a folder has no app ID yet, and the icon is named
    /// after it — so at open there is nothing to look for. If nobody looks again
    /// once the ID is typed, an icon that is already in place never reaches the
    /// plan, and the build installs no icon at all.
    #[test]
    fn an_icon_already_in_place_is_taken_up_once_the_app_id_is_known() {
        let dir = tempfile::tempdir().unwrap();
        let apps = dir.path().join("icons/hicolor/scalable/apps");
        fs::create_dir_all(&apps).unwrap();
        fs::write(apps.join("no.oyzmo.Sample.svg"), b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();

        let mut project = crate::project::Project::from_folder(dir.path()).0;
        assert!(project.manifest.app_id.is_empty());

        // Nothing to find while the ID is empty.
        adopt_existing(&mut project);
        assert!(project.icon_source.is_none());

        // And it is found the moment there is one.
        project.manifest.app_id = "no.oyzmo.Sample".into();
        adopt_existing(&mut project);
        assert_eq!(
            project.icon_source.as_deref(),
            Some(apps.join("no.oyzmo.Sample.svg").as_path())
        );

        // A picture the user chose themselves is never overruled.
        project.icon_source = Some(dir.path().join("mine.svg"));
        adopt_existing(&mut project);
        assert_eq!(project.icon_source.as_deref(), Some(dir.path().join("mine.svg").as_path()));
    }

    #[test]
    fn installing_puts_it_in_the_right_place_and_leaves_the_original_alone() {
        let dir = tempfile::tempdir().unwrap();
        let source = write(dir.path(), "my-drawing.png", &png_bytes(128, 128));
        let project = dir.path().join("project");

        let installed = install(&source, &project, "no.oyzmo.Thing").unwrap();
        assert_eq!(
            installed.path,
            Path::new("icons/hicolor/128x128/apps/no.oyzmo.Thing.png")
        );
        assert!(project.join(&installed.path).is_file());
        assert!(source.is_file(), "the original is not moved");
    }

    #[test]
    fn installing_the_file_that_is_already_in_place_does_not_empty_it() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().to_path_buf();
        let relative = target_path("no.oyzmo.Thing", &Kind::Svg);
        let target = project.join(&relative);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();

        install(&target, &project, "no.oyzmo.Thing").unwrap();
        assert!(!fs::read(&target).unwrap().is_empty());
    }
}
