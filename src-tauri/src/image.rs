use crate::llm::LlmMessage;
use serde::Deserialize;

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

/// Build messages asking the model for an SVG illustration of the selected
/// passage, returned as JSON `{ "svg": "...", "caption": "..." }`.
pub fn build_image_messages(
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
        "You generate SVG illustrations for educational content for a reader at \
         {} reading level.{}\n\n\
         Given a highlighted passage and its surrounding paragraph, create a \
         clear SVG that helps the reader visualize what it describes.\n\n\
         Return ONLY a JSON object with this shape (no markdown, no code fence):\n\
         {{\"plan\": \"...\", \"svg\": \"<svg viewBox=\\\"...\\\">…</svg>\", \"caption\": \"short description\"}}\n\n\
         Fill \"plan\" FIRST: in 2-4 sentences, describe the composition before \
         you draw — the key elements, how they're arranged, and their approximate \
         positions and sizes within the viewBox coordinate space. Then draw the \
         SVG to match that plan; this planning step markedly improves the result.\n\n\
         SVG requirements:\n\
         - Must include a viewBox attribute.\n\
         - Use only basic shapes (rect, circle, ellipse, line, path, polygon, polyline) and a small palette.\n\
         - Transparent background (no full-canvas background rectangle).\n\
         - Use stroke=\"currentColor\" where lines should adapt to the page text color.\n\
         - No <image>, <foreignObject>, <script>, external references, or embedded fonts.\n\
         - Small text labels are allowed where they aid understanding.\n\
         - Keep it compact (under ~6 kB).",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Highlighted passage: {selection}\n\nSurrounding paragraph: {context}\n\nGenerate the SVG."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Build messages asking the model to revise an existing SVG per the user's
/// instruction, returned as the same JSON `{ "svg", "caption" }` shape.
pub fn build_regenerate_messages(
    source: &str,
    current_svg: &str,
    instruction: &str,
    context: &str,
    reading_level: &str,
    book_topic: Option<&str>,
) -> Vec<LlmMessage> {
    let topic_clause = match book_topic {
        Some(t) if !t.is_empty() => format!("\nThe book's subject is: {t}.\n"),
        _ => String::new(),
    };
    let context_clause = if context.trim().is_empty() {
        String::new()
    } else {
        format!("\n\nSurrounding paragraph: {context}")
    };

    let system = format!(
        "You revise SVG illustrations for educational content for a reader at \
         {} reading level.{}\n\n\
         You are given an existing illustration and a requested change. Return \
         a revised version that applies the change while keeping the same \
         general subject.\n\n\
         Return ONLY a JSON object (no markdown, no code fence):\n\
         {{\"plan\": \"...\", \"svg\": \"<svg viewBox=\\\"...\\\">…</svg>\", \"caption\": \"short description\"}}\n\n\
         Fill \"plan\" FIRST: in 2-4 sentences, describe how the revised \
         composition will look and what changes from the current image, then \
         draw the SVG to match that plan.\n\n\
         Same SVG rules as before: a viewBox attribute; only basic shapes; \
         transparent background; stroke=\"currentColor\" where lines should \
         adapt to the page text color; no <image>, <foreignObject>, <script>, \
         external references, or embedded fonts; compact (under ~6 kB). It is \
         fine for the new image to have a different aspect ratio if the change \
         calls for it.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "The illustration is about: {source}{context_clause}\n\nCurrent SVG:\n{current_svg}\n\nRequested change: {instruction}\n\nReturn the revised SVG."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

#[derive(Debug, Deserialize)]
pub struct GeneratedImage {
    pub svg: String,
    #[serde(default)]
    pub caption: String,
}

/// Extract the JSON object from the model response (tolerating code fences or
/// surrounding prose) and parse it into a GeneratedImage.
pub fn parse_image_response(response: &str) -> Result<GeneratedImage, String> {
    let json = extract_json(response);
    let img: GeneratedImage = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse image response: {e}"))?;
    if !img.svg.trim_start().starts_with("<svg") {
        return Err("response did not contain an <svg> element".to_string());
    }
    Ok(img)
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

/// Compute the width/height aspect ratio from the SVG viewBox. Falls back to
/// 1.0 (square) when the viewBox is missing or unparseable.
pub fn aspect_ratio_of(svg: &str) -> f32 {
    let needle = "viewBox";
    if let Some(pos) = svg.find(needle) {
        let after = &svg[pos + needle.len()..];
        // Skip whitespace and '=' to the opening quote.
        if let Some(q_rel) = after.find(['"', '\'']) {
            let quote = after.as_bytes()[q_rel] as char;
            let rest = &after[q_rel + 1..];
            if let Some(end) = rest.find(quote) {
                let value = &rest[..end];
                let parts: Vec<f32> = value
                    .split([' ', ','])
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| p.parse::<f32>().ok())
                    .collect();
                if parts.len() == 4 && parts[3] > 0.0 {
                    return parts[2] / parts[3];
                }
            }
        }
    }
    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_includes_selection_and_context() {
        let msgs = build_image_messages(
            "gravitational collapse",
            "A star collapses under its own gravity.",
            "adult",
            None,
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("gravitational collapse"));
        assert!(user.content.contains("collapses under its own gravity"));
    }

    #[test]
    fn prompt_includes_reading_level_and_topic() {
        let msgs = build_image_messages("x", "y", "child", Some("Black holes"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("child (ages 8–12)"));
        assert!(system.content.contains("Black holes"));
    }

    #[test]
    fn parse_plain_json() {
        let r = r#"{"svg": "<svg viewBox=\"0 0 10 10\"></svg>", "caption": "a box"}"#;
        let img = parse_image_response(r).unwrap();
        assert!(img.svg.contains("<svg"));
        assert_eq!(img.caption, "a box");
    }

    #[test]
    fn parse_json_in_code_fence() {
        let r = "```json\n{\"svg\": \"<svg viewBox=\\\"0 0 1 1\\\"></svg>\", \"caption\": \"c\"}\n```";
        let img = parse_image_response(r).unwrap();
        assert!(img.svg.contains("<svg"));
    }

    #[test]
    fn parse_rejects_non_svg() {
        let r = r#"{"svg": "not an svg", "caption": "x"}"#;
        assert!(parse_image_response(r).is_err());
    }

    #[test]
    fn parse_ignores_plan_field() {
        let r = r#"{"plan": "a circle then a box", "svg": "<svg viewBox=\"0 0 10 10\"></svg>", "caption": "c"}"#;
        let img = parse_image_response(r).unwrap();
        assert!(img.svg.contains("<svg"));
        assert_eq!(img.caption, "c");
    }

    #[test]
    fn image_prompt_requests_a_plan_first() {
        let msgs = build_image_messages("x", "y", "adult", None);
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("plan"));
        assert!(system.content.contains("\"plan\""));
    }

    #[test]
    fn aspect_ratio_wide() {
        let svg = r#"<svg viewBox="0 0 100 50"></svg>"#;
        assert_eq!(aspect_ratio_of(svg), 2.0);
    }

    #[test]
    fn aspect_ratio_tall() {
        let svg = r#"<svg viewBox="0 0 50 100"></svg>"#;
        assert_eq!(aspect_ratio_of(svg), 0.5);
    }

    #[test]
    fn aspect_ratio_handles_commas() {
        let svg = r#"<svg viewBox="0,0,200,100"></svg>"#;
        assert_eq!(aspect_ratio_of(svg), 2.0);
    }

    #[test]
    fn aspect_ratio_defaults_to_square_without_viewbox() {
        assert_eq!(aspect_ratio_of("<svg></svg>"), 1.0);
    }

    #[test]
    fn regenerate_prompt_includes_instruction_and_current_svg() {
        let msgs = build_regenerate_messages(
            "a black hole",
            "<svg viewBox=\"0 0 10 10\"></svg>",
            "make it more colorful",
            "Black holes warp spacetime.",
            "adult",
            Some("Black holes"),
        );
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("make it more colorful"));
        assert!(user.content.contains("viewBox"));
        assert!(user.content.contains("a black hole"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Black holes"));
    }

    #[test]
    fn regenerate_prompt_omits_context_clause_when_empty() {
        let msgs = build_regenerate_messages("s", "<svg/>", "change", "", "adult", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Surrounding paragraph"));
    }
}
