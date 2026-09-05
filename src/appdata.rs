//! The two files that make a Flatpak look like an app rather than a binary: the
//! desktop entry that puts it in the menu, and the AppStream metainfo that puts
//! it in a store.
//!
//! Both are named after the app ID, and so is the icon. That rule is the single
//! most common thing beginners get wrong — the app builds, installs, and then
//! has no icon and no name — so the filenames are derived here and never typed.

use crate::oars;
use crate::project::Project;

/// The freedesktop categories, with the words people actually use for them. The
/// first one is what shows in a menu; the rest narrow it down.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("AudioVideo", "Sound and video"),
    ("Audio", "Sound"),
    ("Video", "Video"),
    ("Development", "Programming"),
    ("Education", "Education"),
    ("Game", "Games"),
    ("Graphics", "Pictures and drawing"),
    ("Network", "Internet"),
    ("Office", "Office and documents"),
    ("Science", "Science"),
    ("Settings", "Settings"),
    ("System", "System tools"),
    ("Utility", "Small tools"),
];

pub fn category_label(id: &str) -> &str {
    CATEGORIES
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, label)| *label)
        .unwrap_or(id)
}

/// `<app-id>.desktop`, `<app-id>.metainfo.xml`, `<app-id>.svg` — all from the
/// one place, so they cannot disagree.
pub fn desktop_file_name(app_id: &str) -> String {
    format!("{app_id}.desktop")
}

pub fn metainfo_file_name(app_id: &str) -> String {
    format!("{app_id}.metainfo.xml")
}

/// The desktop entry. `Exec` gets the command as written; `Icon` and
/// `StartupWMClass` get the app ID, which is what makes the window match its
/// launcher instead of showing a blank icon in the task switcher.
pub fn desktop_file(project: &Project) -> String {
    let mut out = String::from("[Desktop Entry]\n");

    push_line(&mut out, "Name", &project.name);
    if !project.summary.trim().is_empty() {
        push_line(&mut out, "Comment", &project.summary);
    }
    push_line(&mut out, "Exec", &project.manifest.command);
    push_line(&mut out, "Icon", &project.manifest.app_id);
    out.push_str("Terminal=false\n");
    out.push_str("Type=Application\n");

    if !project.categories.is_empty() {
        // Trailing semicolon included: the spec calls these lists, and a missing
        // one is the sort of thing desktop-file-validate complains about.
        push_line(
            &mut out,
            "Categories",
            &format!("{};", project.categories.join(";")),
        );
    }
    if !project.keywords.is_empty() {
        push_line(
            &mut out,
            "Keywords",
            &format!("{};", project.keywords.join(";")),
        );
    }

    out.push_str("StartupNotify=true\n");
    push_line(&mut out, "StartupWMClass", &project.manifest.app_id);
    out
}

fn push_line(out: &mut String, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    // Desktop files are one line per key; a stray newline would silently produce
    // a broken entry rather than an error.
    let value = value.replace(['\n', '\r'], " ");
    out.push_str(key);
    out.push('=');
    out.push_str(&value);
    out.push('\n');
}

/// The AppStream metainfo. Everything a store shows: name, summary, the longer
/// description as paragraphs, licence, developer, screenshots, the content
/// rating, and at least one release.
/// Whether this listing is one the build will accept.
///
/// flatpak-builder runs `appstreamcli compose` over the app information file it
/// finds installed, and that program is strict: no summary, no description or no
/// category and it refuses the listing — which fails the whole build, at the very
/// end, with a one-word code. Each of the three was measured on its own against
/// flatpak-builder 1.4.10 and the GNOME 50 SDK.
///
/// The file is always written; this decides whether it is *installed into the
/// app*, so an unfinished listing costs the listing rather than the build.
pub fn listing_is_complete(project: &Project) -> bool {
    !project.summary.trim().is_empty()
        && !project.description.trim().is_empty()
        && !project.categories.is_empty()
}

pub fn metainfo(project: &Project) -> String {
    let app_id = project.manifest.app_id.trim();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<component type=\"desktop-application\">\n");

    element(&mut out, 1, "id", app_id);
    element(&mut out, 1, "metadata_license", "CC0-1.0");
    if !project.license.trim().is_empty() {
        element(&mut out, 1, "project_license", project.license.trim());
    }
    out.push('\n');

    element(&mut out, 1, "name", &project.name);
    if !project.summary.trim().is_empty() {
        element(&mut out, 1, "summary", project.summary.trim());
    }

    let paragraphs: Vec<&str> = project
        .description
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .collect();
    if !paragraphs.is_empty() {
        out.push_str("\n  <description>\n");
        for paragraph in paragraphs {
            let text = paragraph.replace('\n', " ");
            element(&mut out, 2, "p", &text);
        }
        out.push_str("  </description>\n");
    }

    out.push('\n');
    out.push_str(&format!(
        "  <launchable type=\"desktop-id\">{}</launchable>\n",
        escape(&desktop_file_name(app_id))
    ));
    if !project.homepage.trim().is_empty() {
        out.push_str(&format!(
            "  <url type=\"homepage\">{}</url>\n",
            escape(project.homepage.trim())
        ));
    }

    if !project.developer.trim().is_empty() {
        let developer_id = developer_id(app_id);
        out.push_str(&format!("\n  <developer id=\"{}\">\n", escape(&developer_id)));
        element(&mut out, 2, "name", project.developer.trim());
        out.push_str("  </developer>\n");
    }

    if !project.screenshots.is_empty() {
        out.push_str("\n  <screenshots>\n");
        for (index, shot) in project.screenshots.iter().enumerate() {
            let default = if index == 0 { " type=\"default\"" } else { "" };
            out.push_str(&format!("    <screenshot{default}>\n"));
            out.push_str(&format!("      <image>{}</image>\n", escape(&shot.url)));
            if !shot.caption.trim().is_empty() {
                element(&mut out, 3, "caption", shot.caption.trim());
            }
            out.push_str("    </screenshot>\n");
        }
        out.push_str("  </screenshots>\n");
    }

    let attributes = oars::attributes(&project.content_rating);
    out.push('\n');
    if attributes.is_empty() {
        // Empty, not absent: absent means "not rated", empty means "nothing to
        // declare", and stores treat those very differently.
        out.push_str("  <content_rating type=\"oars-1.1\"/>\n");
    } else {
        out.push_str("  <content_rating type=\"oars-1.1\">\n");
        for (id, value) in attributes {
            out.push_str(&format!(
                "    <content_attribute id=\"{id}\">{value}</content_attribute>\n"
            ));
        }
        out.push_str("  </content_rating>\n");
    }

    let version = project.release_version.trim();
    if !version.is_empty() {
        out.push_str("\n  <releases>\n");
        out.push_str(&format!(
            "    <release version=\"{}\" date=\"{}\">\n",
            escape(version),
            escape(project.release_date.trim())
        ));
        let notes = project.release_notes.trim();
        if !notes.is_empty() {
            out.push_str("      <description>\n");
            element(&mut out, 4, "p", notes);
            out.push_str("      </description>\n");
        }
        out.push_str("    </release>\n");
        out.push_str("  </releases>\n");
    }

    out.push_str("</component>\n");
    out
}

/// AppStream wants the developer's own reverse-DNS id, and the app's domain is
/// the only one we can know: `no.oyzmo.PackItFlat` gives `no.oyzmo`.
fn developer_id(app_id: &str) -> String {
    let parts: Vec<&str> = app_id.split('.').collect();
    if parts.len() >= 3 {
        parts[..parts.len() - 1].join(".")
    } else {
        app_id.to_string()
    }
}

fn element(out: &mut String, depth: usize, name: &str, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    out.push_str(&"  ".repeat(depth));
    out.push_str(&format!("<{name}>{}</{name}>\n", escape(text)));
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Today, as AppStream writes dates. Through GLib because pulling in a date
/// crate for one line would have to be vendored for the offline build.
pub fn today() -> String {
    gtk::glib::DateTime::now_local()
        .and_then(|now| now.format("%Y-%m-%d"))
        .map(|date| date.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;
    use crate::project::Screenshot;

    fn project() -> Project {
        let import = manifest::parse_str(
            "app-id: no.oyzmo.PackItFlat\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: packitflat\n",
        )
        .unwrap();
        let mut project = Project::from_import(&import, None);
        project.name = "Pack It Flat".into();
        project.summary = "Make a Flatpak without the jargon".into();
        project.description = "Turns a program into a Flatpak.\n\nAsks plain questions.".into();
        project.license = "GPL-3.0-or-later".into();
        project.developer = "oyzmo".into();
        project.homepage = "http://oyzmo.no".into();
        project.categories = vec!["Development".into(), "Utility".into()];
        project.keywords = vec!["flatpak".into(), "manifest".into()];
        project.release_version = "0.3.0".into();
        project.release_date = "2026-09-04".into();
        project.release_notes = "First public release.".into();
        project
    }

    #[test]
    fn the_desktop_entry_names_everything_after_the_app_id() {
        let text = desktop_file(&project());
        assert!(text.starts_with("[Desktop Entry]\n"));
        assert!(text.contains("Name=Pack It Flat\n"));
        assert!(text.contains("Exec=packitflat\n"));
        assert!(text.contains("Icon=no.oyzmo.PackItFlat\n"));
        assert!(text.contains("StartupWMClass=no.oyzmo.PackItFlat\n"));
        assert!(text.contains("Categories=Development;Utility;\n"));
        assert!(text.contains("Keywords=flatpak;manifest;\n"));
        assert_eq!(desktop_file_name("no.oyzmo.PackItFlat"), "no.oyzmo.PackItFlat.desktop");
    }

    #[test]
    fn a_multi_line_summary_cannot_break_the_desktop_entry() {
        let mut project = project();
        project.summary = "Line one\nLine two".into();
        let text = desktop_file(&project);
        assert!(text.contains("Comment=Line one Line two\n"));
        // One key per line, still.
        assert!(text.lines().all(|line| line == "[Desktop Entry]" || line.contains('=')));
    }

    #[test]
    fn empty_fields_are_left_out_rather_than_written_blank() {
        let mut project = project();
        project.summary.clear();
        project.categories.clear();
        let text = desktop_file(&project);
        assert!(!text.contains("Comment="));
        assert!(!text.contains("Categories="));
    }

    #[test]
    fn the_metainfo_has_what_a_store_needs() {
        let text = metainfo(&project());
        assert!(text.contains("<id>no.oyzmo.PackItFlat</id>"));
        assert!(text.contains("<project_license>GPL-3.0-or-later</project_license>"));
        assert!(text.contains("<metadata_license>CC0-1.0</metadata_license>"));
        assert!(text.contains("<launchable type=\"desktop-id\">no.oyzmo.PackItFlat.desktop</launchable>"));
        assert!(text.contains("<developer id=\"no.oyzmo\">"));
        assert!(text.contains("<p>Turns a program into a Flatpak.</p>"));
        assert!(text.contains("<p>Asks plain questions.</p>"));
        assert!(text.contains("<release version=\"0.3.0\" date=\"2026-09-04\">"));
    }

    #[test]
    fn nothing_to_declare_is_written_as_an_empty_rating_not_a_missing_one() {
        let text = metainfo(&project());
        assert!(text.contains("<content_rating type=\"oars-1.1\"/>"));
    }

    #[test]
    fn declared_content_becomes_attributes() {
        let mut project = project();
        crate::oars::set_answer(&mut project.content_rating, "violence", 1);
        let text = metainfo(&project);
        assert!(text.contains("<content_attribute id=\"violence-cartoon\">moderate</content_attribute>"));
    }

    #[test]
    fn text_that_would_break_the_xml_is_escaped() {
        let mut project = project();
        project.name = "Bits & <Bobs>".into();
        project.summary = "It's \"useful\"".into();
        let text = metainfo(&project);
        assert!(text.contains("<name>Bits &amp; &lt;Bobs&gt;</name>"));
        assert!(text.contains("&quot;useful&quot;"));
        assert!(!text.contains("<Bobs>"));
    }

    #[test]
    fn screenshots_mark_the_first_one_as_the_default() {
        let mut project = project();
        project.screenshots = vec![
            Screenshot {
                url: "https://example.org/one.png".into(),
                caption: "The welcome page".into(),
            },
            Screenshot {
                url: "https://example.org/two.png".into(),
                caption: String::new(),
            },
        ];
        let text = metainfo(&project);
        assert!(text.contains("<screenshot type=\"default\">"));
        assert!(text.contains("<caption>The welcome page</caption>"));
        assert_eq!(text.matches("<screenshot").count(), 3); // two entries plus the wrapper
    }

    #[test]
    fn a_project_with_nothing_filled_in_still_produces_valid_looking_files() {
        let import = manifest::parse_str("app-id: no.oyzmo.Bare\n").unwrap();
        let bare = Project::from_import(&import, None);

        let desktop = desktop_file(&bare);
        assert!(desktop.starts_with("[Desktop Entry]\n"));
        assert!(desktop.contains("Type=Application\n"));

        let metainfo = metainfo(&bare);
        assert!(metainfo.starts_with("<?xml"));
        assert!(metainfo.ends_with("</component>\n"));
        assert!(!metainfo.contains("<releases>"));
    }

    #[test]
    fn categories_are_offered_by_the_names_people_use() {
        assert_eq!(category_label("Game"), "Games");
        assert_eq!(category_label("Development"), "Programming");
        assert_eq!(category_label("Unknown"), "Unknown");
    }
}
