use crate::llm::LlmMessage;

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

pub fn build_messages(
    word: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You write brief, focused glosses of words for a learner reading at \
         {} reading level.{}\n\n\
         Given a word and the sentence it appears in, return a single short \
         definition that fits THIS specific usage — not a general dictionary \
         entry. Pitch the language to the reader's level.\n\n\
         Rules:\n\
         - One or two sentences. Hard maximum: 40 words.\n\
         - Match the meaning to the surrounding context. If the word has \
           multiple senses, pick the one used here.\n\
         - Plain prose only. No quotes around the word, no \"Definition:\" \
           prefix, no markdown, no formatting.\n\
         - Do not repeat the surrounding sentence back. Just define.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Word: {word}\n\nSentence: {context}\n\nDefine \"{word}\" as used in this sentence."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Strip wrapping quotes and any "Definition:" / "<word>:" prefixes the model
/// may have prepended despite instructions. Defensive cleanup only.
pub fn clean_response(word: &str, raw: &str) -> String {
    let mut s = raw.trim().to_string();

    let prefixes = [
        format!("{word}:"),
        format!("\"{word}\":"),
        "Definition:".to_string(),
        "definition:".to_string(),
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
    fn includes_word_and_context_in_user_message() {
        let msgs = build_messages("blackhole", "A blackhole bends light.", "adult", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("blackhole"));
        assert!(user.content.contains("A blackhole bends light."));
    }

    #[test]
    fn includes_reading_level_in_system_prompt() {
        let cases = [
            ("child", "child (ages 8–12)"),
            ("teen", "teen (ages 13–17)"),
            ("adult", "general adult"),
            ("academic", "professional or postgraduate"),
        ];
        for (level, expected) in cases {
            let msgs = build_messages("x", "y", level, None);
            let system = msgs.iter().find(|m| m.role == "system").unwrap();
            assert!(
                system.content.contains(expected),
                "system prompt for '{level}' should contain '{expected}'"
            );
        }
    }

    #[test]
    fn includes_book_topic_when_provided() {
        let msgs = build_messages("x", "y", "adult", Some("Black holes"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Black holes"));
    }

    #[test]
    fn omits_topic_clause_when_no_topic() {
        let msgs = build_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(!system.content.contains("The book's subject is"));
    }

    #[test]
    fn caps_response_length_instruction_is_present() {
        let msgs = build_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("40 words"));
    }

    #[test]
    fn clean_response_strips_leading_word_colon() {
        let r = clean_response("blackhole", "blackhole: a region of spacetime where gravity is strong.");
        assert_eq!(r, "a region of spacetime where gravity is strong.");
    }

    #[test]
    fn clean_response_strips_definition_prefix() {
        let r = clean_response("x", "Definition: a thing.");
        assert_eq!(r, "a thing.");
    }

    #[test]
    fn clean_response_strips_wrapping_quotes() {
        let r = clean_response("x", "\"a thing.\"");
        assert_eq!(r, "a thing.");
    }

    #[test]
    fn clean_response_passes_through_plain_text() {
        let r = clean_response("x", "A region of spacetime.");
        assert_eq!(r, "A region of spacetime.");
    }
}
