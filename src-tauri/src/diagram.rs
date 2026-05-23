use crate::llm::LlmMessage;
use serde::Deserialize;

const SYSTEM_PROMPT: &str = "You design clear, well-organized educational diagrams \
    in SVG format. Choose the diagram type (flowchart, sequence diagram, mind map, \
    hierarchy/tree, state machine, network diagram, timeline, Venn diagram, \
    comparison table, ER diagram, decision tree, bar chart, etc.) that best fits \
    the domain and content, then draw it.";

const EVAL_SYSTEM_PROMPT: &str = "You evaluate educational diagrams that accompany \
    text. Be honest and concise.";

/// Maximum evaluate-and-improve iterations.
pub const MAX_ITERATIONS: usize = 5;
/// Both scores (help, ease) must reach this value on the 1–5 scale for the
/// loop to exit early as "good enough".
pub const SATISFACTORY_SCORE: u8 = 4;
/// Total render passes displayed to the user: one initial render plus the
/// improvement budget.
pub const TOTAL_PASSES: usize = MAX_ITERATIONS + 1;

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

/// Compose the diagram in prose: pick the diagram type and describe its
/// structure. Carries the book's original learning prompt (when set) so the
/// model picks a type that fits the field.
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

/// Author the initial SVG from the composition. Text-only.
pub fn build_initial_render_messages(composition: &str) -> Vec<LlmMessage> {
    let user = format!(
        "Composition:\n{composition}\n\nGenerate the SVG diagram. Return only the SVG."
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Vision call asking the evaluator to rate the rendered diagram against the
/// highlighted text on two 5-point scales.
pub fn build_evaluation_messages(source: &str) -> Vec<LlmMessage> {
    let user = format!(
        "Highlighted text:\n{source}\n\nThe attached image is a diagram meant to help a \
         reader understand the text above. Rate it on two 5-point scales:\n\
         1. Help: how much does this diagram help a human understand the text? \
         (1 = doesn't help, 5 = makes it much clearer)\n\
         2. Ease: how easy is this diagram for a human to understand? \
         (1 = confusing, 5 = immediately clear)\n\n\
         Return ONLY a JSON object (no markdown, no code fence):\n\
         {{\"help\": 1-5, \"ease\": 1-5, \"feedback\": \"specific suggestions to improve the diagram\"}}\n\n\
         If the diagram is already excellent, feedback can be brief or empty."
    );
    vec![
        LlmMessage { role: "system".to_string(), content: EVAL_SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Text call asking the generator to produce an improved SVG given the
/// previous one, the evaluator's scores, and any feedback.
pub fn build_improve_messages(
    composition: &str,
    current_svg: &str,
    score: &DiagramScore,
) -> Vec<LlmMessage> {
    let feedback_clause = if score.feedback.trim().is_empty() {
        String::new()
    } else {
        format!("\nFeedback: {}", score.feedback.trim())
    };
    let user = format!(
        "Composition:\n{composition}\n\nCurrent SVG:\n{current_svg}\n\nEvaluation:\n\
         - Help: {}/5\n- Ease: {}/5{}\n\n\
         Produce an improved SVG that addresses the feedback and raises both scores. \
         Return only the SVG.",
        score.help, score.ease, feedback_clause
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

#[derive(Debug, Deserialize)]
pub struct DiagramScore {
    pub help: u8,
    pub ease: u8,
    #[serde(default)]
    pub feedback: String,
}

impl DiagramScore {
    pub fn is_satisfactory(&self) -> bool {
        self.help >= SATISFACTORY_SCORE && self.ease >= SATISFACTORY_SCORE
    }
}

pub fn parse_diagram_score(response: &str) -> Result<DiagramScore, String> {
    let json = extract_json(response);
    serde_json::from_str(&json).map_err(|e| format!("failed to parse diagram score: {e}"))
}

fn extract_json(s: &str) -> String {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|rest| rest.trim_start())
        .and_then(|rest| rest.strip_suffix("```"))
        .map(|rest| rest.trim())
        .unwrap_or(trimmed);
    match (inner.find('{'), inner.rfind('}')) {
        (Some(start), Some(end)) if end > start => inner[start..=end].to_string(),
        _ => inner.to_string(),
    }
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
        let msgs = build_composition_messages("x", "", "adult", Some(""));
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Domain"));
    }

    #[test]
    fn composition_includes_reading_level() {
        let msgs = build_composition_messages("x", "", "teen", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("teen (ages 13–17)"));
    }

    #[test]
    fn evaluation_prompt_asks_for_two_scales_and_json() {
        let msgs = build_evaluation_messages("the Krebs cycle");
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Help"));
        assert!(user.content.contains("Ease"));
        assert!(user.content.contains("5-point"));
        assert!(user.content.contains("\"help\""));
        assert!(user.content.contains("\"ease\""));
        assert!(user.content.contains("\"feedback\""));
        assert!(user.content.contains("the Krebs cycle"));
    }

    #[test]
    fn improve_prompt_includes_svg_scores_and_feedback() {
        let score = DiagramScore {
            help: 3,
            ease: 2,
            feedback: "Labels overlap; reorder boxes left to right".to_string(),
        };
        let msgs = build_improve_messages(
            "A flowchart of cellular respiration",
            "<svg viewBox=\"0 0 10 10\"></svg>",
            &score,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("3/5"));
        assert!(user.content.contains("2/5"));
        assert!(user.content.contains("Labels overlap"));
        assert!(user.content.contains("Current SVG"));
        assert!(user.content.contains("viewBox"));
    }

    #[test]
    fn improve_prompt_omits_feedback_section_when_empty() {
        let score = DiagramScore { help: 4, ease: 5, feedback: String::new() };
        let msgs = build_improve_messages("c", "<svg/>", &score);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Feedback:"));
    }

    #[test]
    fn parse_score_plain_json() {
        let r = r#"{"help": 4, "ease": 3, "feedback": "Tighten spacing"}"#;
        let s = parse_diagram_score(r).unwrap();
        assert_eq!(s.help, 4);
        assert_eq!(s.ease, 3);
        assert_eq!(s.feedback, "Tighten spacing");
        assert!(!s.is_satisfactory()); // ease < 4
    }

    #[test]
    fn parse_score_in_code_fence() {
        let r = "```json\n{\"help\": 5, \"ease\": 4}\n```";
        let s = parse_diagram_score(r).unwrap();
        assert!(s.is_satisfactory());
        assert_eq!(s.feedback, "");
    }

    #[test]
    fn parse_score_in_surrounding_prose() {
        let r = "Sure, my evaluation: {\"help\": 2, \"ease\": 2, \"feedback\": \"x\"} done.";
        let s = parse_diagram_score(r).unwrap();
        assert_eq!(s.help, 2);
        assert_eq!(s.ease, 2);
        assert!(!s.is_satisfactory());
    }

    #[test]
    fn satisfactory_requires_both_at_threshold() {
        assert!(DiagramScore { help: 5, ease: 4, feedback: String::new() }.is_satisfactory());
        assert!(DiagramScore { help: 4, ease: 4, feedback: String::new() }.is_satisfactory());
        assert!(!DiagramScore { help: 5, ease: 3, feedback: String::new() }.is_satisfactory());
        assert!(!DiagramScore { help: 3, ease: 5, feedback: String::new() }.is_satisfactory());
    }
}
