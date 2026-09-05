//! The content rating, asked as questions rather than as OARS identifiers.
//!
//! App stores want an age-rating block, and the official form of it is a list of
//! identifiers like `violence-bloodshed=intense`. Nobody packaging a text editor
//! should have to read that list, so this asks six plain questions and works the
//! identifiers out afterwards. Answering nothing gives the same result as
//! answering "no" to everything, which is the honest default for the apps this
//! tool is for.

/// One answer to one question, and the identifiers it implies.
pub struct Answer {
    pub label: &'static str,
    pub attributes: &'static [(&'static str, &'static str)],
}

pub struct Question {
    /// Stored with the project, so re-ordering the questions can't scramble
    /// somebody's saved answers.
    pub id: &'static str,
    pub text: &'static str,
    pub help: &'static str,
    /// The first answer is always the harmless one, and the default.
    pub answers: &'static [Answer],
}

pub const QUESTIONS: &[Question] = &[
    Question {
        id: "violence",
        text: "Does the app show violence?",
        help: "Fighting, weapons, injury — in pictures or in words.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "Cartoon or fantasy violence",
                attributes: &[("violence-cartoon", "moderate")],
            },
            Answer {
                label: "Realistic violence",
                attributes: &[("violence-realistic", "moderate")],
            },
            Answer {
                label: "Graphic violence, with blood",
                attributes: &[
                    ("violence-realistic", "intense"),
                    ("violence-bloodshed", "intense"),
                ],
            },
        ],
    },
    Question {
        id: "sex",
        text: "Does it show nudity or sexual content?",
        help: "Including artwork and photographs.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "Suggestive, but nothing explicit",
                attributes: &[("sex-themes", "mild")],
            },
            Answer {
                label: "Nudity",
                attributes: &[("sex-nudity", "moderate")],
            },
            Answer {
                label: "Explicit sexual content",
                attributes: &[("sex-nudity", "intense"), ("sex-themes", "intense")],
            },
        ],
    },
    Question {
        id: "language",
        text: "Does it use bad language?",
        help: "Swearing, insults, or slurs — in the app's own text or in what people write in it.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "Mild swearing",
                attributes: &[("language-profanity", "mild")],
            },
            Answer {
                label: "Frequent or strong swearing",
                attributes: &[("language-profanity", "intense")],
            },
        ],
    },
    Question {
        id: "drugs",
        text: "Does it mention alcohol, tobacco or drugs?",
        help: "Mentioning counts; so does showing someone using them.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "They are mentioned",
                attributes: &[("drugs-alcohol", "mild")],
            },
            Answer {
                label: "Their use is shown",
                attributes: &[
                    ("drugs-alcohol", "moderate"),
                    ("drugs-narcotics", "moderate"),
                ],
            },
        ],
    },
    Question {
        id: "money",
        text: "Can people spend money in it?",
        help: "Adverts, things to buy inside the app, or anything you can gamble with.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "It shows adverts",
                attributes: &[("money-advertising", "moderate")],
            },
            Answer {
                label: "It sells things inside the app",
                attributes: &[("money-purchasing", "intense")],
            },
            Answer {
                label: "It has gambling",
                attributes: &[("money-gambling", "intense")],
            },
        ],
    },
    Question {
        id: "social",
        text: "Can people reach each other through it?",
        help: "Chat, voice, or anything that shares who or where someone is.",
        answers: &[
            Answer {
                label: "No",
                attributes: &[],
            },
            Answer {
                label: "They can send each other messages",
                attributes: &[("social-chat", "intense")],
            },
            Answer {
                label: "They can talk or use video",
                attributes: &[("social-chat", "intense"), ("social-audio", "intense")],
            },
            Answer {
                label: "It shares where people are",
                attributes: &[("social-location", "intense")],
            },
        ],
    },
];

pub fn question(id: &str) -> Option<&'static Question> {
    QUESTIONS.iter().find(|question| question.id == id)
}

/// The answer index stored for a question, defaulting to the harmless one.
pub fn answer_index(answers: &[(String, usize)], id: &str) -> usize {
    answers
        .iter()
        .find(|(question, _)| question == id)
        .map(|(_, index)| *index)
        .unwrap_or(0)
}

pub fn set_answer(answers: &mut Vec<(String, usize)>, id: &str, index: usize) {
    match answers.iter_mut().find(|(question, _)| question == id) {
        Some((_, slot)) => *slot = index,
        None => answers.push((id.to_string(), index)),
    }
}

/// The identifiers for a set of answers, sorted so the generated file doesn't
/// churn. An empty list means "nothing objectionable", which AppStream writes as
/// an empty content_rating element rather than as no element at all.
pub fn attributes(answers: &[(String, usize)]) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&str, &str)> = Vec::new();

    for question in QUESTIONS {
        let index = answer_index(answers, question.id);
        let Some(answer) = question.answers.get(index) else {
            continue;
        };
        for (id, value) in answer.attributes {
            if !out.iter().any(|(existing, _)| existing == id) {
                out.push((id, value));
            }
        }
    }

    out.sort_by_key(|(id, _)| *id);
    out
}

/// A sentence for the review step: what the answers add up to.
pub fn summary(answers: &[(String, usize)]) -> String {
    let attributes = attributes(answers);
    if attributes.is_empty() {
        return "Nothing that needs an age rating.".to_string();
    }

    let flagged: Vec<&str> = QUESTIONS
        .iter()
        .filter(|question| answer_index(answers, question.id) > 0)
        .filter_map(|question| {
            question
                .answers
                .get(answer_index(answers, question.id))
                .map(|answer| answer.label)
        })
        .collect();
    format!("Rated for: {}.", flagged.join("; ").to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answering_nothing_means_nothing_to_declare() {
        assert!(attributes(&[]).is_empty());
        assert_eq!(summary(&[]), "Nothing that needs an age rating.");
    }

    #[test]
    fn the_first_answer_is_always_the_harmless_one() {
        for question in QUESTIONS {
            assert_eq!(question.answers[0].label, "No", "{}", question.id);
            assert!(
                question.answers[0].attributes.is_empty(),
                "{} declares something for “No”",
                question.id
            );
            assert!(question.answers.len() >= 2);
            assert!(!question.help.is_empty());
        }
    }

    #[test]
    fn answers_become_the_identifiers_stores_expect() {
        let mut answers = Vec::new();
        set_answer(&mut answers, "violence", 3);
        set_answer(&mut answers, "social", 1);

        let attributes = attributes(&answers);
        assert!(attributes.contains(&("violence-bloodshed", "intense")));
        assert!(attributes.contains(&("violence-realistic", "intense")));
        assert!(attributes.contains(&("social-chat", "intense")));
        // Sorted, so regenerating the file doesn't shuffle it.
        let ids: Vec<&str> = attributes.iter().map(|(id, _)| *id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn changing_an_answer_replaces_it_rather_than_adding_one() {
        let mut answers = Vec::new();
        set_answer(&mut answers, "money", 1);
        set_answer(&mut answers, "money", 3);
        assert_eq!(answers.len(), 1);
        assert_eq!(answer_index(&answers, "money"), 3);
        assert!(attributes(&answers).contains(&("money-gambling", "intense")));
    }

    #[test]
    fn out_of_range_answers_are_ignored_not_fatal() {
        let answers = vec![("violence".to_string(), 99)];
        assert!(attributes(&answers).is_empty());
    }

    #[test]
    fn the_summary_lists_what_was_declared() {
        let mut answers = Vec::new();
        set_answer(&mut answers, "language", 1);
        assert_eq!(summary(&answers), "Rated for: mild swearing.");
    }
}
