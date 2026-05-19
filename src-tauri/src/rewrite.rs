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

/// Build messages for the back-and-forth conversation that opens when the
/// learner clicks "I still don't understand" on an already-rewritten passage.
/// The system prompt scopes the model to a tutoring role and forbids
/// proposing edits (the rewrite happens separately when the learner is
/// ready). `history` is the full prior conversation including the latest
/// user message — the LLM returns just the next assistant turn.
pub fn build_conversation_messages(
    passage: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
    history: Vec<LlmMessage>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You're a patient tutor for a learner reading at {} reading level.{}\n\n\
         The learner is reading the passage below and has already asked for a \
         simpler version, but they still don't understand. Help them by \
         answering their questions and clarifying concepts. Use language and \
         examples pitched at their reading level.\n\n\
         Passage they're struggling with:\n{}\n\n\
         Surrounding paragraph: {}\n\n\
         Rules:\n\
         - Be concise and focused. 1 to 3 sentences per turn is usually right.\n\
         - Don't suggest rewrites or modifications — the learner will request \
           a rewrite separately when they're ready.\n\
         - If they ask multiple questions in one turn, address each briefly.",
        reading_level_description(reading_level),
        topic_clause,
        passage,
        context,
    );

    let mut messages = vec![LlmMessage { role: "system".to_string(), content: system }];
    messages.extend(history);
    messages
}

/// Build messages for the post-conversation rewrite: produce a clearer
/// version of the passage that incorporates the concepts and explanations
/// from the tutoring conversation.
pub fn build_conversation_rewrite_messages(
    passage: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
    history: Vec<LlmMessage>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let transcript: String = history
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let system = format!(
        "You rewrite passages from educational content for a learner reading at \
         {} reading level.{}\n\n\
         The learner has been struggling with the passage below. A tutor \
         conversation about what was confusing is included — write a clearer \
         version of the passage that incorporates the concepts and \
         explanations that helped them in the conversation.\n\n\
         Passage to rewrite:\n{}\n\n\
         Surrounding paragraph: {}\n\n\
         Tutor conversation:\n{}\n\n\
         Rules:\n\
         - Return ONLY the rewritten passage as plain prose.\n\
         - No \"Here is...\" prefix. No quotes. No markdown.\n\
         - Reads as a drop-in replacement in the surrounding paragraph.\n\
         - Pitch language to the reader's level.",
        reading_level_description(reading_level),
        topic_clause,
        passage,
        context,
        transcript,
    );

    let user = "Write the improved version of the passage.".to_string();

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

    #[test]
    fn conversation_prompt_pins_tutor_role_and_passage() {
        let history = vec![LlmMessage {
            role: "user".to_string(),
            content: "What does 'collapse' mean here?".to_string(),
        }];
        let msgs = build_conversation_messages(
            "gravitational collapse",
            "When a massive star runs out of fuel, gravitational collapse begins.",
            "adult",
            None,
            history,
        );
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("tutor"));
        assert!(system.content.contains("gravitational collapse"));
        assert!(system.content.contains("Don't suggest rewrites"));
        // History is preserved after the system message.
        assert_eq!(msgs[1].role, "user");
        assert!(msgs[1].content.contains("collapse"));
    }

    #[test]
    fn conversation_rewrite_prompt_includes_transcript() {
        let history = vec![
            LlmMessage { role: "user".to_string(), content: "What is collapse?".to_string() },
            LlmMessage { role: "assistant".to_string(), content: "It's when matter falls inward.".to_string() },
        ];
        let msgs = build_conversation_rewrite_messages(
            "gravitational collapse",
            "When a massive star runs out of fuel, gravitational collapse begins.",
            "adult",
            None,
            history,
        );
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("matter falls inward"));
        assert!(system.content.contains("gravitational collapse"));
        assert!(system.content.contains("plain prose"));
    }

    #[test]
    fn conversation_rewrite_prompt_drops_system_messages_from_transcript() {
        let history = vec![
            LlmMessage { role: "system".to_string(), content: "internal".to_string() },
            LlmMessage { role: "user".to_string(), content: "Q?".to_string() },
        ];
        let msgs = build_conversation_rewrite_messages("p", "c", "adult", None, history);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(!system.content.contains("internal"));
        assert!(system.content.contains("Q?"));
    }
}
