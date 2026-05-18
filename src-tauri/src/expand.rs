use crate::llm::LlmMessage;

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

/// Build messages for a footnote-length expansion of the highlighted text.
/// A footnote should be brief — a few sentences that add useful context to
/// what the passage is already saying, without restating it.
pub fn build_footnote_messages(
    selection: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You write brief footnote-style asides for a learner reading at \
         {} reading level.{}\n\n\
         Given a highlighted passage and its surrounding paragraph, write a \
         footnote that adds context, nuance, or background to what's being \
         said. Pitch the language to the reader's level.\n\n\
         Rules:\n\
         - 2 to 3 sentences. Hard maximum: 80 words.\n\
         - Add information; do not restate the highlighted passage.\n\
         - Plain prose. No \"Footnote:\" prefix, no markdown, no formatting, \
           no quotes around the passage.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Highlighted passage: {selection}\n\nSurrounding paragraph: {context}\n\nWrite the footnote."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Build messages for an endnote-length expansion of the highlighted text.
/// An endnote sits between a footnote and an appendix in length — a few
/// paragraphs covering the topic with more depth than a footnote.
pub fn build_endnote_messages(
    selection: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You write endnote-length expansions of highlighted passages for a \
         learner reading at {} reading level.{}\n\n\
         Given a highlighted passage and its surrounding paragraph, write \
         an endnote that goes deeper than a brief footnote but stops short \
         of being a full appendix — typically 2 to 3 short paragraphs that \
         expand context, history, or technical detail.\n\n\
         Rules:\n\
         - 2 to 3 short paragraphs. Hard maximum: 250 words.\n\
         - Add substantive information; do not restate the highlight.\n\
         - Plain markdown prose. No \"Endnote:\" prefix, no quotes around \
           the passage, no closing recommendations (those belong in an \
           appendix, not here).\n\
         - Pitch language to the reader's level.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Highlighted passage: {selection}\n\nSurrounding paragraph: {context}\n\nWrite the endnote."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Build messages for an appendix-length expansion: a multi-paragraph chapter
/// that ends with a "Further reading" section of recommendations at the
/// reader's level.
pub fn build_appendix_messages(
    selection: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };

    let system = format!(
        "You write appendix chapters expanding on highlighted topics for a \
         learner reading at {} reading level.{}\n\n\
         Given a highlighted passage and the paragraph it appears in, write \
         a multi-paragraph appendix that explores the topic in depth.\n\n\
         The appendix MUST end with a \"## Further reading\" section \
         recommending 2 to 4 books or academic papers appropriate for the \
         reader's level. Annotate each recommendation with one sentence \
         describing why it's worth reading.\n\n\
         Rules:\n\
         - Markdown format. Use ## for section headings inside the appendix.\n\
         - 3 to 6 paragraphs of body content before the recommendations.\n\
         - Do NOT include an H1 title — the chapter title is set elsewhere.\n\
         - The final section MUST be \"## Further reading\" with annotated \
           recommendations.\n\
         - Pitch language to the reader's level.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Highlighted passage: {selection}\n\nSurrounding paragraph: {context}\n\nWrite the appendix."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Strip code fences and a leading H1 (the chapter title is set externally
/// via the manifest, so an H1 inside the body is redundant noise).
pub fn clean_appendix_response(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for prefix in ["```markdown\n", "```md\n", "```\n"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            if let Some(end) = rest.rfind("```") {
                s = rest[..end].trim_end().to_string();
                break;
            }
        }
    }
    let lines: Vec<&str> = s.lines().collect();
    if lines.first().map(|l| l.starts_with("# ")).unwrap_or(false) {
        s = lines[1..].join("\n").trim_start().to_string();
    }
    s.trim().to_string()
}

pub fn clean_endnote_response(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for p in ["Endnote:", "endnote:", "Note:", "note:"] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.trim_start().to_string();
            break;
        }
    }
    s.trim().to_string()
}

/// Strip wrapping quotes and common "Footnote:" / "Note:" prefixes the model
/// may add despite instructions.
pub fn clean_footnote_response(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for p in ["Footnote:", "footnote:", "Note:", "note:"] {
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
    fn includes_selection_and_context_in_user_message() {
        let msgs = build_footnote_messages(
            "gravitational collapse",
            "When a massive star runs out of fuel, gravitational collapse happens.",
            "adult",
            None,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("gravitational collapse"));
        assert!(user.content.contains("runs out of fuel"));
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
            let msgs = build_footnote_messages("x", "y", level, None);
            let system = msgs.iter().find(|m| m.role == "system").unwrap();
            assert!(
                system.content.contains(expected),
                "footnote prompt for '{level}' should contain '{expected}'"
            );
        }
    }

    #[test]
    fn includes_book_topic_when_provided() {
        let msgs = build_footnote_messages("x", "y", "adult", Some("Black holes"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Black holes"));
    }

    #[test]
    fn caps_response_length_instruction_is_present() {
        let msgs = build_footnote_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("80 words"));
    }

    #[test]
    fn clean_response_strips_footnote_prefix() {
        let r = clean_footnote_response("Footnote: a thing happened.");
        assert_eq!(r, "a thing happened.");
    }

    #[test]
    fn clean_response_strips_wrapping_quotes() {
        let r = clean_footnote_response("\"a thing happened.\"");
        assert_eq!(r, "a thing happened.");
    }

    #[test]
    fn clean_response_passes_through_plain_text() {
        let r = clean_footnote_response("A region of spacetime.");
        assert_eq!(r, "A region of spacetime.");
    }

    #[test]
    fn endnote_prompt_includes_selection_and_context() {
        let msgs = build_endnote_messages(
            "Hawking radiation",
            "Stephen Hawking proposed that black holes emit radiation.",
            "adult",
            None,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Hawking radiation"));
        assert!(user.content.contains("Stephen Hawking proposed"));
    }

    #[test]
    fn endnote_prompt_caps_at_250_words() {
        let msgs = build_endnote_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("250 words"));
    }

    #[test]
    fn endnote_prompt_forbids_further_reading_section() {
        let msgs = build_endnote_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("appendix"));
    }

    #[test]
    fn clean_endnote_strips_prefix() {
        assert_eq!(clean_endnote_response("Endnote: a thing."), "a thing.");
    }

    #[test]
    fn appendix_prompt_requires_further_reading_section() {
        let msgs = build_appendix_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Further reading"));
    }

    #[test]
    fn appendix_prompt_includes_selection_and_context() {
        let msgs = build_appendix_messages("Hawking radiation", "When black holes evaporate.", "adult", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Hawking radiation"));
        assert!(user.content.contains("When black holes evaporate"));
    }

    #[test]
    fn appendix_prompt_forbids_h1_inside_body() {
        let msgs = build_appendix_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("H1"));
    }

    #[test]
    fn clean_appendix_strips_code_fence() {
        let raw = "```markdown\nSome content here.\n```";
        assert_eq!(clean_appendix_response(raw), "Some content here.");
    }

    #[test]
    fn clean_appendix_strips_leading_h1() {
        let raw = "# Big Title\n\nThe body of the appendix.";
        assert_eq!(clean_appendix_response(raw), "The body of the appendix.");
    }

    #[test]
    fn clean_appendix_passes_through_plain_markdown() {
        let raw = "## Section\n\nA paragraph.\n\n## Further reading\n\n- A book.";
        assert_eq!(clean_appendix_response(raw), raw);
    }
}
