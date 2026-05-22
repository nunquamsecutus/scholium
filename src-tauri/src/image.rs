use crate::llm::LlmMessage;
use crate::settings::ImageQuality;
use serde::Deserialize;

/// The target each quality level is judged against during refinement.
pub fn quality_description(q: &ImageQuality) -> &'static str {
    match q {
        ImageQuality::Fast => {
            "a quick rough sketch: simple shapes that convey the basic idea; \
             rough proportions and minor flaws are acceptable"
        }
        ImageQuality::Medium => {
            "a clear, tidy illustration: elements are recognizable, proportions \
             are reasonable, and there is no obvious error or clutter"
        }
        ImageQuality::High => {
            "a polished, detailed illustration: accurate proportions, a clean and \
             balanced composition, refined detail, and clear labels where helpful"
        }
    }
}

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

/// Build messages for one evaluate-and-maybe-improve pass. The rendered PNG is
/// attached separately by the vision call. The model judges the rendering
/// against `quality` and either declares it good enough or returns an improved
/// SVG.
pub fn build_evaluate_messages(
    source: &str,
    context: &str,
    quality: &str,
    caption: &str,
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
        format!("\nSurrounding paragraph: {context}")
    };

    let system = format!(
        "You evaluate and improve SVG illustrations for educational content for a \
         reader at {} reading level.{}\n\n\
         You are shown the current rendering of an illustration, the intent it \
         should convey, and a target quality bar. Judge honestly whether the \
         rendering already meets the target quality.\n\n\
         Return ONLY a JSON object (no markdown, no code fence):\n\
         {{\"meets\": true|false, \"plan\": \"...\", \"svg\": \"<svg viewBox=\\\"...\\\">…</svg>\", \"caption\": \"...\"}}\n\n\
         - If it already meets the target, return {{\"meets\": true}} and nothing else.\n\
         - If it does NOT meet the target, return \"meets\": false, put your \
           critique and the specific fixes in \"plan\", and provide an improved \
           \"svg\" and \"caption\".\n\n\
         SVG rules: a viewBox; only basic shapes; transparent background; \
         stroke=\"currentColor\" where lines should adapt to the page text color; \
         no <image>, <foreignObject>, <script>, external references, or embedded \
         fonts; compact.",
        reading_level_description(reading_level),
        topic_clause,
    );

    let user = format!(
        "Intent: an illustration of {source}.{context_clause}\nCurrent caption: {caption}\n\n\
         Target quality: {quality}.\n\n\
         The attached image is the current rendering. Judge whether it meets the \
         target quality, and improve it if it does not."
    );

    vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Render an SVG to PNG bytes, scaled so the long edge is ~512px. Loads system
/// fonts so any <text> renders. Used to feed the vision critique loop.
pub fn rasterize_svg_to_png(svg: &str) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};

    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("svg parse failed: {e}"))?;

    let size = tree.size();
    let max_edge = size.width().max(size.height());
    if max_edge <= 0.0 {
        return Err("svg has no usable size".to_string());
    }
    let scale = 512.0 / max_edge;
    let w = ((size.width() * scale).ceil() as u32).max(1);
    let h = ((size.height() * scale).ceil() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("failed to allocate pixmap")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| format!("png encode failed: {e}"))
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

/// The result of an evaluate-and-maybe-improve pass. When `meets` is true the
/// rendering passed the quality bar and `svg` is typically absent; otherwise
/// `svg`/`caption` carry the improved illustration.
#[derive(Debug, Deserialize)]
pub struct ImageEvaluation {
    pub meets: bool,
    #[serde(default)]
    pub svg: Option<String>,
    #[serde(default)]
    pub caption: String,
}

pub fn parse_image_evaluation(response: &str) -> Result<ImageEvaluation, String> {
    let json = extract_json(response);
    serde_json::from_str(&json).map_err(|e| format!("failed to parse image evaluation: {e}"))
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

    #[test]
    fn evaluate_prompt_includes_quality_target_and_meets_field() {
        let msgs = build_evaluate_messages(
            "a black hole",
            "",
            "a polished, detailed illustration",
            "a black hole",
            "adult",
            None,
        );
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("\"meets\""));
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Target quality: a polished, detailed illustration"));
        assert!(user.content.contains("attached image"));
    }

    #[test]
    fn quality_descriptions_differ_by_level() {
        let fast = quality_description(&ImageQuality::Fast);
        let medium = quality_description(&ImageQuality::Medium);
        let high = quality_description(&ImageQuality::High);
        assert!(fast.contains("rough"));
        assert!(medium.contains("tidy"));
        assert!(high.contains("polished"));
    }

    #[test]
    fn parse_evaluation_meets_true() {
        let eval = parse_image_evaluation(r#"{"meets": true}"#).unwrap();
        assert!(eval.meets);
        assert!(eval.svg.is_none());
    }

    #[test]
    fn parse_evaluation_with_improvement() {
        let r = r#"{"meets": false, "plan": "fix it", "svg": "<svg viewBox=\"0 0 1 1\"></svg>", "caption": "c"}"#;
        let eval = parse_image_evaluation(r).unwrap();
        assert!(!eval.meets);
        assert_eq!(eval.svg.as_deref(), Some("<svg viewBox=\"0 0 1 1\"></svg>"));
        assert_eq!(eval.caption, "c");
    }

    #[test]
    fn rasterize_produces_png_bytes() {
        let svg = r#"<svg viewBox="0 0 100 50" xmlns="http://www.w3.org/2000/svg"><rect width="100" height="50" fill="blue"/></svg>"#;
        let png = rasterize_svg_to_png(svg).unwrap();
        assert!(png.len() > 8);
        // PNG magic number.
        assert_eq!(&png[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn rasterize_rejects_garbage() {
        assert!(rasterize_svg_to_png("not an svg at all").is_err());
    }
}
