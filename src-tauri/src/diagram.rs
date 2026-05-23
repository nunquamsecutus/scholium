use crate::llm::LlmMessage;

const SYSTEM_PROMPT: &str = "You design clear, well-organized educational diagrams \
    in SVG format. Choose the diagram type (flowchart, sequence diagram, mind map, \
    hierarchy/tree, state machine, network diagram, timeline, Venn diagram, \
    comparison table, ER diagram, decision tree, bar chart, etc.) that best fits \
    the domain and content, then draw it.";

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

/// Compose the diagram in prose: pick the diagram type and describe its
/// structure. The user's original learning prompt (when set) carries the
/// domain so the model picks a type that fits the field.
pub fn build_composition_messages(
    source: &str,
    context: &str,
    reading_level: &str,
    original_prompt: Option<&str>,
) -> Vec<LlmMessage> {
    let mut user = format!("Reading level: {}.", reading_level_description(reading_level));
    if let Some(p) = original_prompt {
        if !p.trim().is_empty() {
            user.push_str(&format!("\nDomain (the learner asked to study): {p}."));
        }
    }
    user.push_str(&format!("\nContent to diagram: {source}."));
    if !context.trim().is_empty() {
        user.push_str(&format!("\nSurrounding paragraph: {context}"));
    }
    user.push_str(
        "\n\nPick the diagram type that best fits the domain and content, then describe \
         the diagram's structure in prose: what nodes/elements it has, how they connect \
         or relate, what each label says, and how they should be arranged. State the \
         diagram type first.",
    );

    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

pub fn build_initial_render_messages(composition: &str) -> Vec<LlmMessage> {
    let user = format!(
        "Composition:\n{composition}\n\nGenerate the SVG diagram. Return only the SVG."
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

pub fn build_polish_messages(composition: &str, critique: Option<&str>) -> Vec<LlmMessage> {
    let mut user = format!("Composition:\n{composition}\n\n");
    if let Some(c) = critique {
        if !c.trim().is_empty() {
            user.push_str(&format!("Critique of the current diagram:\n{c}\n\n"));
        }
    }
    user.push_str(
        "The attached image is the current rendering. Polish it to make it clearer, \
         better organized, and more readable. Return only the improved SVG.",
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

pub fn build_critique_messages() -> Vec<LlmMessage> {
    let user = "Critique the attached diagram. Describe how to make it clearer, better \
                organized, and more readable.";
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user.to_string() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_asks_to_pick_diagram_type_first() {
        let msgs = build_composition_messages("photosynthesis", "", "adult", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.to_lowercase().contains("pick the diagram type"));
        assert!(user.content.contains("State the diagram type first"));
    }

    #[test]
    fn composition_includes_domain_when_prompt_is_set() {
        let msgs = build_composition_messages(
            "photosynthesis",
            "",
            "adult",
            Some("How plants make food from sunlight"),
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Domain"));
        assert!(user.content.contains("How plants make food from sunlight"));
    }

    #[test]
    fn composition_omits_domain_when_prompt_is_empty() {
        let msgs = build_composition_messages("x", "", "adult", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Domain"));
        let msgs_empty_some = build_composition_messages("x", "", "adult", Some(""));
        let user = msgs_empty_some.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Domain"));
    }

    #[test]
    fn composition_includes_reading_level() {
        let cases = [
            ("child", "child (ages 8–12)"),
            ("teen", "teen (ages 13–17)"),
            ("adult", "general adult"),
            ("academic", "professional or postgraduate"),
        ];
        for (level, expected) in cases {
            let msgs = build_composition_messages("x", "", level, None);
            let user = msgs.iter().find(|m| m.role == "user").unwrap();
            assert!(
                user.content.contains(expected),
                "diagram composition for '{level}' should mention '{expected}'"
            );
        }
    }

    #[test]
    fn composition_includes_content_and_context() {
        let msgs = build_composition_messages(
            "the Krebs cycle",
            "Cells generate ATP through a series of reactions.",
            "adult",
            None,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("the Krebs cycle"));
        assert!(user.content.contains("ATP"));
    }

    #[test]
    fn polish_prompt_includes_critique_when_present() {
        let msgs = build_polish_messages("A flowchart with three boxes", Some("the arrows are too thin"));
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("arrows are too thin"));
        assert!(user.content.contains("three boxes"));
    }

    #[test]
    fn polish_prompt_omits_critique_section_when_none() {
        let msgs = build_polish_messages("a diagram", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Critique"));
    }
}
