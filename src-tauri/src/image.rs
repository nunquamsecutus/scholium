use crate::llm::{LlmMessage, CLAUDE_HAIKU_MODEL, CLAUDE_OPUS_MODEL, CLAUDE_SONNET_MODEL};
use crate::settings::ImageQuality;

/// One polish phase: a run of N render iterations, optionally preceded by a
/// fresh critique that's threaded into every iteration's prompt.
#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub iterations: usize,
    pub critique_first: bool,
}

/// A full image pipeline for one quality level. Composition is text-only and
/// describes the layout in prose; the first render produces the initial SVG
/// from that prose; subsequent iterations polish the rendered image.
#[derive(Debug, Clone)]
pub struct ImagePipeline {
    pub composition_model: &'static str,
    pub first_render_model: &'static str,
    pub polish_model: &'static str,
    pub critique_model: &'static str,
    pub steps: Vec<PipelineStep>,
}

pub fn pipeline_for(quality: &ImageQuality) -> ImagePipeline {
    match quality {
        ImageQuality::Fast => ImagePipeline {
            composition_model: CLAUDE_SONNET_MODEL,
            first_render_model: CLAUDE_HAIKU_MODEL,
            polish_model: CLAUDE_HAIKU_MODEL,
            critique_model: CLAUDE_SONNET_MODEL,
            steps: vec![PipelineStep { iterations: 5, critique_first: false }],
        },
        ImageQuality::Medium => ImagePipeline {
            composition_model: CLAUDE_SONNET_MODEL,
            first_render_model: CLAUDE_HAIKU_MODEL,
            polish_model: CLAUDE_HAIKU_MODEL,
            critique_model: CLAUDE_SONNET_MODEL,
            steps: vec![
                PipelineStep { iterations: 5, critique_first: false },
                PipelineStep { iterations: 5, critique_first: true },
            ],
        },
        ImageQuality::High => ImagePipeline {
            composition_model: CLAUDE_OPUS_MODEL,
            first_render_model: CLAUDE_SONNET_MODEL,
            polish_model: CLAUDE_HAIKU_MODEL,
            critique_model: CLAUDE_SONNET_MODEL,
            steps: vec![
                PipelineStep { iterations: 5, critique_first: false },
                PipelineStep { iterations: 5, critique_first: true },
                PipelineStep { iterations: 5, critique_first: true },
            ],
        },
    }
}

pub fn total_iterations(pipeline: &ImagePipeline) -> usize {
    pipeline.steps.iter().map(|s| s.iterations).sum()
}

const SYSTEM_PROMPT: &str = "You are making professional quality art in SVG format.";

/// Compose the illustration in prose. No SVG yet — just describe the layout,
/// the elements, their arrangement, and their relative sizes.
pub fn build_composition_messages(
    source: &str,
    context: &str,
    extra_instruction: Option<&str>,
) -> Vec<LlmMessage> {
    let mut user = format!("Describe the composition for an illustration of: {source}.");
    if !context.trim().is_empty() {
        user.push_str(&format!("\n\nSurrounding paragraph: {context}"));
    }
    if let Some(extra) = extra_instruction {
        if !extra.trim().is_empty() {
            user.push_str(&format!("\n\nAdditional direction: {extra}"));
        }
    }
    user.push_str("\n\nReturn the composition as prose.");

    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Author the initial SVG from the composition. Text-only — no image yet.
pub fn build_initial_render_messages(composition: &str) -> Vec<LlmMessage> {
    let user = format!(
        "Composition:\n{composition}\n\nGenerate the SVG illustration. Return only the SVG."
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Polish the current rendering. The PNG is attached by the vision call.
/// When a critique is available it's woven into the prompt; otherwise the
/// instruction is just to make the image more professional.
pub fn build_polish_messages(composition: &str, critique: Option<&str>) -> Vec<LlmMessage> {
    let mut user = format!("Composition:\n{composition}\n\n");
    if let Some(c) = critique {
        if !c.trim().is_empty() {
            user.push_str(&format!("Critique of the current image:\n{c}\n\n"));
        }
    }
    user.push_str(
        "The attached image is the current rendering. Polish it to make it look more \
         professional. Return only the improved SVG.",
    );
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user },
    ]
}

/// Critique the current rendering. The PNG is attached by the vision call.
pub fn build_critique_messages() -> Vec<LlmMessage> {
    let user = "Critique the attached image. Describe how to make it more professional.";
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: user.to_string() },
    ]
}

/// Extract an `<svg …>…</svg>` block from a model response, tolerating
/// surrounding prose or code fences.
pub fn extract_svg(response: &str) -> Result<String, String> {
    let start = response
        .find("<svg")
        .ok_or("response did not contain an <svg> element")?;
    let after = &response[start..];
    let close = after
        .find("</svg>")
        .ok_or("response had <svg> but no closing tag")?;
    Ok(response[start..start + close + "</svg>".len()].to_string())
}

/// Trim a composition down to something usable as an image caption / alt
/// text. First sentence if short enough, otherwise a hard truncation.
pub fn derive_caption(composition: &str) -> String {
    let trimmed = composition.trim();
    if let Some(end) = trimmed.find(['.', '!', '?']) {
        let sentence = trimmed[..end].trim();
        if !sentence.is_empty() && sentence.chars().count() <= 80 {
            return sentence.to_string();
        }
    }
    let mut out: String = trimmed.chars().take(77).collect();
    if trimmed.chars().count() > 77 {
        out.push('…');
    }
    out
}

#[derive(Debug)]
pub struct GeneratedImage {
    pub svg: String,
    pub caption: String,
}

/// Render an SVG to PNG bytes, scaled so the long edge is ~512px. Loads system
/// fonts so any <text> renders. Used to feed the vision pipeline.
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

/// Compute the width/height aspect ratio from the SVG viewBox. Falls back to
/// 1.0 (square) when the viewBox is missing or unparseable.
pub fn aspect_ratio_of(svg: &str) -> f32 {
    let needle = "viewBox";
    if let Some(pos) = svg.find(needle) {
        let after = &svg[pos + needle.len()..];
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
    fn pipeline_fast_has_one_phase_of_five() {
        let p = pipeline_for(&ImageQuality::Fast);
        assert_eq!(total_iterations(&p), 5);
        assert_eq!(p.steps.len(), 1);
        assert!(!p.steps[0].critique_first);
        assert_eq!(p.composition_model, CLAUDE_SONNET_MODEL);
        assert_eq!(p.first_render_model, CLAUDE_HAIKU_MODEL);
        assert_eq!(p.polish_model, CLAUDE_HAIKU_MODEL);
    }

    #[test]
    fn pipeline_medium_has_two_phases_with_critique() {
        let p = pipeline_for(&ImageQuality::Medium);
        assert_eq!(total_iterations(&p), 10);
        assert_eq!(p.steps.len(), 2);
        assert!(!p.steps[0].critique_first);
        assert!(p.steps[1].critique_first);
        assert_eq!(p.composition_model, CLAUDE_SONNET_MODEL);
    }

    #[test]
    fn pipeline_high_uses_opus_and_three_phases() {
        let p = pipeline_for(&ImageQuality::High);
        assert_eq!(total_iterations(&p), 15);
        assert_eq!(p.steps.len(), 3);
        assert_eq!(p.composition_model, CLAUDE_OPUS_MODEL);
        assert_eq!(p.first_render_model, CLAUDE_SONNET_MODEL);
        assert_eq!(p.polish_model, CLAUDE_HAIKU_MODEL);
        assert!(!p.steps[0].critique_first);
        assert!(p.steps[1].critique_first);
        assert!(p.steps[2].critique_first);
    }

    #[test]
    fn composition_prompt_includes_subject_and_context() {
        let msgs = build_composition_messages("a black hole", "Black holes warp spacetime.", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("a black hole"));
        assert!(user.content.contains("Black holes warp spacetime"));
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("professional"));
    }

    #[test]
    fn composition_prompt_includes_extra_instruction() {
        let msgs = build_composition_messages("a thing", "", Some("make it blue"));
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("make it blue"));
    }

    #[test]
    fn polish_prompt_includes_critique_when_present() {
        let msgs = build_polish_messages("a star and orbit", Some("the orbit is lopsided"));
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("lopsided"));
        assert!(user.content.contains("a star and orbit"));
    }

    #[test]
    fn polish_prompt_omits_critique_section_when_none() {
        let msgs = build_polish_messages("a star", None);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(!user.content.contains("Critique"));
    }

    #[test]
    fn extract_svg_finds_svg_block() {
        let r = "Sure, here it is:\n<svg viewBox=\"0 0 10 10\"><circle/></svg>\nThat's it.";
        let svg = extract_svg(r).unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
    }

    #[test]
    fn extract_svg_errors_when_missing() {
        assert!(extract_svg("just prose, no svg").is_err());
    }

    #[test]
    fn derive_caption_takes_first_sentence_when_short() {
        let c = "A black hole at the center. Lots of surrounding stars.";
        assert_eq!(derive_caption(c), "A black hole at the center");
    }

    #[test]
    fn derive_caption_truncates_when_no_short_sentence() {
        let long = "a ".repeat(100);
        let cap = derive_caption(&long);
        assert!(cap.chars().count() <= 78);
        assert!(cap.ends_with('…'));
    }

    #[test]
    fn aspect_ratio_wide() {
        assert_eq!(aspect_ratio_of(r#"<svg viewBox="0 0 100 50"></svg>"#), 2.0);
    }

    #[test]
    fn aspect_ratio_defaults_to_square_without_viewbox() {
        assert_eq!(aspect_ratio_of("<svg></svg>"), 1.0);
    }

    #[test]
    fn rasterize_produces_png_bytes() {
        let svg = r#"<svg viewBox="0 0 100 50" xmlns="http://www.w3.org/2000/svg"><rect width="100" height="50" fill="blue"/></svg>"#;
        let png = rasterize_svg_to_png(svg).unwrap();
        assert!(png.len() > 8);
        assert_eq!(&png[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    }

    #[test]
    fn rasterize_rejects_garbage() {
        assert!(rasterize_svg_to_png("not an svg at all").is_err());
    }
}
