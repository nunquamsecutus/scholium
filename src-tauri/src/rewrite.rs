use crate::llm::LlmMessage;

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

/// Build messages for a first-pass rewrite: make the highlighted passage
/// easier to understand at the reader's level while preserving meaning.
pub fn build_rewrite_messages(
    passage: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You rewrite passages from educational content for a learner reading at \
         {} reading level.{}\n\n\
         Given a passage and its surrounding paragraph, return a clearer \
         version that:\n\
         - Uses simpler vocabulary and sentence structure.\n\
         - Preserves the technical accuracy and key ideas.\n\
         - Reads naturally as a drop-in replacement in the surrounding paragraph.\n\
         - Is approximately the same length as the original.\n\n\
         Rules:\n\
         - Return ONLY the rewritten passage as plain prose.\n\
         - No \"Here is...\" prefix. No quotes around the passage. No markdown.\n\
         - Pitch language to the reader's level.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Passage to rewrite: {passage}\n\nSurrounding paragraph: {context}\n\nRewrite the passage."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Strip common "Here is..." / "Rewrite:" prefixes and wrapping quotes the
/// model may add despite instructions.
pub fn clean_rewrite_response(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    let prefixes = [
        "Here is the rewritten passage:",
        "Here's the rewritten passage:",
        "Here is a clearer version:",
        "Here is the rewrite:",
        "Rewritten passage:",
        "Rewrite:",
    ];
    for p in &prefixes {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.trim_start().to_string();
            break;
        }
    }
    s = s.trim().to_string();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\u{201C}') && s.ends_with('\u{201D}'))
    {
        s = s[1..s.len() - 1].to_string();
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_passage_and_context() {
        let msgs = build_rewrite_messages(
            "gravitational collapse",
            "When a massive star runs out of fuel, gravitational collapse begins.",
            "adult",
            None,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("gravitational collapse"));
        assert!(user.content.contains("runs out of fuel"));
    }

    #[test]
    fn prompt_includes_reading_level() {
        let cases = [
            ("child", "child (ages 8–12)"),
            ("teen", "teen (ages 13–17)"),
            ("adult", "general adult"),
            ("academic", "professional or postgraduate"),
        ];
        for (level, expected) in cases {
            let msgs = build_rewrite_messages("x", "y", level, None);
            let system = msgs.iter().find(|m| m.role == "system").unwrap();
            assert!(
                system.content.contains(expected),
                "rewrite prompt for '{level}' should contain '{expected}'"
            );
        }
    }

    #[test]
    fn prompt_includes_book_topic_when_provided() {
        let msgs = build_rewrite_messages("x", "y", "adult", Some("Black holes"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Black holes"));
    }

    #[test]
    fn prompt_demands_plain_prose_output() {
        let msgs = build_rewrite_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("plain prose"));
        assert!(system.content.contains("No markdown"));
    }

    #[test]
    fn clean_response_strips_here_is_prefix() {
        assert_eq!(
            clean_rewrite_response("Here is the rewritten passage: a thing."),
            "a thing."
        );
    }

    #[test]
    fn clean_response_strips_wrapping_quotes() {
        assert_eq!(clean_rewrite_response("\"a thing.\""), "a thing.");
    }

    #[test]
    fn clean_response_passes_through_plain_text() {
        assert_eq!(
            clean_rewrite_response("A region of spacetime."),
            "A region of spacetime."
        );
    }
}
