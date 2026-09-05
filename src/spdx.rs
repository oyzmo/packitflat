//! The licence list.
//!
//! App stores want an SPDX identifier — a standard short name — and beginners
//! type "GPLv3", which isn't one. So the picker searches on both the identifier
//! and the everyday name, and each entry carries a sentence saying what the
//! licence actually lets people do.
//!
//! This is the common subset, not all ~600 SPDX identifiers: a searchable list
//! of forty licences people actually pick is more useful than a complete one
//! nobody can navigate. `is_known` is deliberately lenient about the rest —
//! anything not on the list is a warning, never an error.

pub struct License {
    /// The SPDX identifier, which is what goes in the metainfo.
    pub id: &'static str,
    /// What people call it.
    pub name: &'static str,
    /// One line: what someone receiving the app may do with it.
    pub summary: &'static str,
}

/// Ordered so the ones most people want come first; the picker keeps this order
/// until the user types something.
pub const LICENSES: &[License] = &[
    License {
        id: "GPL-3.0-or-later",
        name: "GNU General Public License v3 or later",
        summary: "Anyone may use and change it, but must share their changes the same way.",
    },
    License {
        id: "GPL-2.0-or-later",
        name: "GNU General Public License v2 or later",
        summary: "The older version of the GPL, still used by many projects.",
    },
    License {
        id: "LGPL-3.0-or-later",
        name: "GNU Lesser General Public License v3 or later",
        summary: "Like the GPL, but other programs may link to it without becoming open.",
    },
    License {
        id: "LGPL-2.1-or-later",
        name: "GNU Lesser General Public License v2.1 or later",
        summary: "The older Lesser GPL, common for libraries.",
    },
    License {
        id: "AGPL-3.0-or-later",
        name: "GNU Affero General Public License v3 or later",
        summary: "Like the GPL, and also covers software people use over a network.",
    },
    License {
        id: "MIT",
        name: "MIT License",
        summary: "Anyone may do almost anything, as long as they keep the copyright notice.",
    },
    License {
        id: "Apache-2.0",
        name: "Apache License 2.0",
        summary: "Permissive like MIT, with an explicit patent grant.",
    },
    License {
        id: "BSD-3-Clause",
        name: "BSD 3-Clause License",
        summary: "Permissive; the project's name may not be used to endorse other work.",
    },
    License {
        id: "BSD-2-Clause",
        name: "BSD 2-Clause License",
        summary: "Permissive, with only the copyright notice to keep.",
    },
    License {
        id: "MPL-2.0",
        name: "Mozilla Public License 2.0",
        summary: "Changes to these files stay open; the rest of a larger program need not.",
    },
    License {
        id: "ISC",
        name: "ISC License",
        summary: "Permissive, the same idea as MIT in fewer words.",
    },
    License {
        id: "EPL-2.0",
        name: "Eclipse Public License 2.0",
        summary: "Changes stay open; commonly used by Java projects.",
    },
    License {
        id: "Unlicense",
        name: "The Unlicense",
        summary: "Given away entirely, with no conditions at all.",
    },
    License {
        id: "CC0-1.0",
        name: "Creative Commons Zero v1.0 Universal",
        summary: "Put in the public domain as far as the law allows.",
    },
    License {
        id: "CC-BY-4.0",
        name: "Creative Commons Attribution 4.0",
        summary: "Anyone may use it if they credit you. Meant for artwork and text.",
    },
    License {
        id: "CC-BY-SA-4.0",
        name: "Creative Commons Attribution-ShareAlike 4.0",
        summary: "Credit you, and share changes the same way. Meant for artwork and text.",
    },
    License {
        id: "Zlib",
        name: "zlib License",
        summary: "Permissive; altered versions must be marked as altered.",
    },
    License {
        id: "AFL-3.0",
        name: "Academic Free License v3.0",
        summary: "Permissive, with a patent grant and an explicit warranty disclaimer.",
    },
    License {
        id: "Artistic-2.0",
        name: "Artistic License 2.0",
        summary: "Permissive; changed versions must be clearly marked. Common in Perl.",
    },
    License {
        id: "BSL-1.0",
        name: "Boost Software License 1.0",
        summary: "Permissive; the notice need not travel with compiled versions.",
    },
    License {
        id: "LGPL-3.0-only",
        name: "GNU Lesser General Public License v3 only",
        summary: "As LGPL-3.0, but later versions of the licence do not apply.",
    },
    License {
        id: "GPL-3.0-only",
        name: "GNU General Public License v3 only",
        summary: "As GPL-3.0, but later versions of the licence do not apply.",
    },
    License {
        id: "GPL-2.0-only",
        name: "GNU General Public License v2 only",
        summary: "As GPL-2.0, but later versions of the licence do not apply.",
    },
    License {
        id: "MIT-0",
        name: "MIT No Attribution",
        summary: "Like MIT, without even the requirement to keep the notice.",
    },
    License {
        id: "0BSD",
        name: "BSD Zero Clause License",
        summary: "Permissive with no conditions whatsoever.",
    },
    License {
        id: "WTFPL",
        name: "Do What The F*ck You Want To Public License",
        summary: "No conditions at all, phrased bluntly.",
    },
    License {
        id: "OFL-1.1",
        name: "SIL Open Font License 1.1",
        summary: "For fonts: free to use and change, but not to sell on their own.",
    },
    License {
        id: "Proprietary",
        name: "Not free software",
        summary: "All rights reserved. App stores for free software will not accept it.",
    },
    License {
        id: "LicenseRef-custom",
        name: "Something else, written by me",
        summary: "Use this only if none of the standard licences fit; most stores will ask why.",
    },
];

/// The everyday spellings people type, mapped to the identifier they mean. Used
/// so searching "gplv3" finds the right entry.
const ALIASES: &[(&str, &str)] = &[
    ("gplv3", "GPL-3.0-or-later"),
    ("gpl3", "GPL-3.0-or-later"),
    ("gpl v3", "GPL-3.0-or-later"),
    ("gplv2", "GPL-2.0-or-later"),
    ("gpl2", "GPL-2.0-or-later"),
    ("lgplv3", "LGPL-3.0-or-later"),
    ("lgplv2", "LGPL-2.1-or-later"),
    ("agplv3", "AGPL-3.0-or-later"),
    ("apache", "Apache-2.0"),
    ("apache2", "Apache-2.0"),
    ("mozilla", "MPL-2.0"),
    ("bsd", "BSD-3-Clause"),
    ("public domain", "CC0-1.0"),
    ("creative commons", "CC-BY-4.0"),
    ("closed source", "Proprietary"),
    ("commercial", "Proprietary"),
];

/// Everything a row showing `displayed` may be found by, as one string.
///
/// Deliberately more than the row shows: "gplv3" is not a substring of
/// "GPL-3.0-or-later — GNU General Public License v3 or later", and typing
/// "gplv3" is the single most likely thing a beginner does. A picker that
/// searched only its own labels would fail at the one job this module exists
/// for. Anything not recognised — the "Not chosen yet" row — matches on itself.
///
/// The picker filters with [`search`]; this is the second opinion
/// `PACKITFLAT_DEV_LICENCE` judges the picker's answer against, arrived at from
/// the *displayed row* rather than from the query. Two ways of deciding the same
/// thing, agreeing, is the evidence — the same reason the crate list has a
/// crosscheck.
pub fn searchable(displayed: &str) -> String {
    let id = displayed.split(" — ").next().unwrap_or(displayed);
    let Some(license) = LICENSES.iter().find(|l| l.id == id) else {
        return displayed.to_lowercase();
    };

    [license.id, license.name]
        .into_iter()
        .chain(
            ALIASES
                .iter()
                .filter(|(_, id)| *id == license.id)
                .map(|(alias, _)| *alias),
        )
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn is_known(id: &str) -> bool {
    LICENSES.iter().any(|l| l.id.eq_ignore_ascii_case(id))
}

pub fn find(id: &str) -> Option<&'static License> {
    LICENSES.iter().find(|l| l.id.eq_ignore_ascii_case(id))
}

/// Search on identifier, everyday name and the common misspellings. An empty
/// query returns the whole list in its curated order.
pub fn search(query: &str) -> Vec<&'static License> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return LICENSES.iter().collect();
    }

    let aliased: Option<&str> = ALIASES
        .iter()
        .find(|(alias, _)| alias.contains(&query) || query.contains(*alias))
        .map(|(_, id)| *id);

    LICENSES
        .iter()
        .filter(|l| {
            l.id.to_lowercase().contains(&query)
                || l.name.to_lowercase().contains(&query)
                || aliased.is_some_and(|id| id == l.id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_choice_is_first() {
        assert_eq!(LICENSES[0].id, "GPL-3.0-or-later");
        assert!(search("").len() == LICENSES.len());
    }

    #[test]
    fn everyday_spellings_find_the_right_licence() {
        assert_eq!(search("gplv3")[0].id, "GPL-3.0-or-later");
        assert_eq!(search("apache")[0].id, "Apache-2.0");
        assert_eq!(search("public domain")[0].id, "CC0-1.0");
    }

    #[test]
    fn searching_matches_identifier_and_name() {
        assert!(search("mit").iter().any(|l| l.id == "MIT"));
        assert!(search("lesser").iter().any(|l| l.id.starts_with("LGPL")));
        assert!(search("zzz").is_empty());
    }

    /// The picker matches on this rather than on the label, so every everyday
    /// spelling has to be in it — a label-only search finds nothing for "gplv3",
    /// which is what a beginner types.
    #[test]
    fn what_the_picker_searches_carries_the_everyday_spellings() {
        let displayed = format!("{} — {}", LICENSES[0].id, LICENSES[0].name);
        let text = searchable(&displayed);

        assert!(text.contains("gpl-3.0-or-later"));
        assert!(text.contains("gnu general public license"));
        assert!(text.contains("gplv3"), "{text}");
        // Another licence's aliases have no business in here.
        assert!(!text.contains("apache"), "{text}");
    }

    #[test]
    fn a_row_that_names_no_licence_matches_on_itself() {
        assert_eq!(searchable("Not chosen yet"), "not chosen yet");
    }

    #[test]
    fn known_ids_are_case_insensitive() {
        assert!(is_known("gpl-3.0-or-later"));
        assert!(!is_known("GPLv3"));
        assert_eq!(find("MIT").unwrap().name, "MIT License");
    }

    #[test]
    fn every_entry_explains_itself() {
        for license in LICENSES {
            assert!(!license.summary.is_empty(), "{} has no summary", license.id);
            assert!(license.summary.ends_with('.'), "{}", license.id);
        }
    }
}
