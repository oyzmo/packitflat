//! Keeping the raw YAML view and the model in step.
//!
//! The rule that matters is the brief's: *if the user's hand-edits can't be
//! parsed, show the error and don't clobber their text.* So the model is never
//! written back over text that is being typed — only text that has been parsed
//! successfully, and only when it actually differs from what the model would
//! produce anyway.
//!
//! The policy lives here, away from the widgets, because "when may I overwrite
//! what someone is typing" is the one decision in the editor that must not be
//! got wrong, and here it can be tested.

use crate::manifest::{self, ManifestError};
use crate::project::Project;

#[derive(Debug)]
pub enum Outcome {
    /// The text parsed, and the model now matches it.
    Applied,
    /// The text parsed to exactly what the model already held.
    Unchanged,
    /// The text is not valid: the model is untouched and so is the text.
    Invalid(ManifestError),
}

impl Outcome {
    pub fn is_invalid(&self) -> bool {
        matches!(self, Outcome::Invalid(_))
    }
}

/// What the model looks like as YAML — what the view shows when it is not being
/// edited.
pub fn text_for(project: &Project) -> String {
    project
        .manifest
        .to_yaml()
        .unwrap_or_else(|err| err.friendly())
}

/// Take what the user typed. On a parse failure nothing is written anywhere, so
/// the half-finished line they are in the middle of survives.
pub fn apply_text(project: &mut Project, text: &str) -> Outcome {
    match manifest::parse_str(text) {
        Ok(import) => {
            if import.manifest == project.manifest {
                Outcome::Unchanged
            } else {
                project.manifest = import.manifest;
                Outcome::Applied
            }
        }
        Err(err) => Outcome::Invalid(err),
    }
}

/// Whether the view's text should be replaced with the model's. False whenever
/// the text already means the same thing — reformatting somebody's spacing
/// while they type would be its own kind of clobbering.
pub fn should_replace(current_text: &str, project: &Project) -> bool {
    match manifest::parse_str(current_text) {
        Ok(import) => import.manifest != project.manifest,
        // Unparseable text is being worked on. Leave it alone; the error bar
        // already says so.
        Err(_) => false,
    }
}

/// Where to put the cursor for a parse error, as (line, column), 1-based. `None`
/// when the parser couldn't say.
pub fn error_position(err: &ManifestError) -> Option<(usize, usize)> {
    match err {
        ManifestError::Syntax {
            line: Some(line),
            column: Some(column),
            ..
        } => Some((*line, *column)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_str;

    fn project() -> Project {
        let import = parse_str(
            "app-id: no.oyzmo.Sample\nruntime: org.gnome.Platform\nruntime-version: '50'\n\
             sdk: org.gnome.Sdk\ncommand: sample\nmodules:\n  - name: sample\n",
        )
        .unwrap();
        Project::from_import(&import, None)
    }

    #[test]
    fn typing_valid_yaml_updates_the_model() {
        let mut project = project();
        let text = text_for(&project).replace("command: sample", "command: renamed");

        assert!(matches!(apply_text(&mut project, &text), Outcome::Applied));
        assert_eq!(project.manifest.command, "renamed");
    }

    #[test]
    fn identical_text_is_not_a_change() {
        let mut project = project();
        let text = text_for(&project);
        assert!(matches!(apply_text(&mut project, &text), Outcome::Unchanged));
    }

    #[test]
    fn broken_yaml_leaves_the_model_exactly_as_it_was() {
        let mut project = project();
        let before = project.manifest.clone();

        let outcome = apply_text(&mut project, "app-id: fine\n  bad: indent\n");
        assert!(outcome.is_invalid());
        assert_eq!(project.manifest, before);

        match outcome {
            Outcome::Invalid(err) => {
                assert!(error_position(&err).is_some(), "the error says where to look");
                assert!(!err.friendly().is_empty());
            }
            other => panic!("expected invalid, got {other:?}"),
        }
    }

    #[test]
    fn the_view_is_never_rewritten_while_the_text_is_unparseable() {
        let project = project();
        assert!(!should_replace("app-id: fine\n  bad: indent\n", &project));
        assert!(!should_replace("", &project));
    }

    #[test]
    fn formatting_differences_do_not_trigger_a_rewrite() {
        let project = project();
        // Same manifest, written differently: quoted differently, keys reordered,
        // extra blank lines. Replacing this would move the user's cursor for
        // nothing.
        let equivalent = "runtime: \"org.gnome.Platform\"\n\napp-id: no.oyzmo.Sample\n\
                          runtime-version: '50'\nsdk: org.gnome.Sdk\n\
                          command: sample\nmodules:\n  - name: sample\n";
        assert!(!should_replace(equivalent, &project));

        // A real difference does.
        let different = equivalent.replace("sample", "other");
        assert!(should_replace(&different, &project));
    }

    #[test]
    fn hand_edits_the_app_cannot_model_survive_the_round_trip() {
        let mut project = project();
        let text = format!("{}\ncleanup:\n  - /include\n", text_for(&project));

        assert!(matches!(apply_text(&mut project, &text), Outcome::Applied));
        assert!(text_for(&project).contains("cleanup"));
        // …and having taken them in, the view is not rewritten to drop them.
        assert!(!should_replace(&text_for(&project), &project));
    }
}
