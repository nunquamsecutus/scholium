use crate::llm::{LlmMessage, CLAUDE_HAIKU_MODEL, CLAUDE_SONNET_MODEL};
// Note: CLAUDE_OPUS_MODEL intentionally not imported; composition and render
// models are now user-configurable via Settings::claude_image_model.
use crate::settings::ImageQuality;

/// One polish phase: a run of N render iterations, optionally preceded by a
/// fresh critique that's threaded into every iteration's prompt.
#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub iterations: usize,
    pub critique_first: bool,
}

/// A full image pipeline for one quality level. Composition and render/polish
/// steps use the user-configured image model (from Settings); only the
/// critique model is fixed here because evaluation cost is kept independent of
/// the generation model choice.
#[derive(Debug, Clone)]
pub struct ImagePipeline {
    pub critique_model: &'static str,
    pub steps: Vec<PipelineStep>,
}

pub fn pipeline_for(quality: &ImageQuality) -> ImagePipeline {
    match quality {
        ImageQuality::Fast => ImagePipeline {
            critique_model: CLAUDE_SONNET_MODEL,
            steps: vec![PipelineStep { iterations: 5, critique_first: false }],
        },
        ImageQuality::Medium => ImagePipeline {
            critique_model: CLAUDE_SONNET_MODEL,
            steps: vec![
                PipelineStep { iterations: 5, critique_first: false },
                PipelineStep { iterations: 5, critique_first: true },
            ],
        },
        ImageQuality::High => ImagePipeline {
            critique_model: CLAUDE_HAIKU_MODEL,
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

/// Build a compact text-to-image prompt for **diffusion models** (e.g. Flux).
///
/// Diffusion models take a short, descriptive prompt — not a multi-paragraph
/// prose composition — so this builds `"<source>, <context>, <instruction>"`
/// as a single `user` message with no `system` turn.
pub fn build_direct_gen_messages(
    source: &str,
    context: &str,
    extra_instruction: Option<&str>,
) -> Vec<LlmMessage> {
    let mut parts: Vec<&str> = vec![source.trim()];
    if !context.trim().is_empty() {
        parts.push(context.trim());
    }
    if let Some(extra) = extra_instruction {
        if !extra.trim().is_empty() {
            parts.push(extra.trim());
        }
    }
    vec![LlmMessage { role: "user".to_string(), content: parts.join(", ") }]
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

/// Extract an image body and its MIME type from a model response.
///
/// Tried in order:
/// 1. SVG block (`<svg>…</svg>`)  → `("image/svg+xml", svg_text)`
/// 2. Data URL embedded anywhere  → `("image/png"` or `"image/jpeg"`, base64_data)`
/// 3. Raw base64 blob whose decoded prefix matches PNG or JPEG magic bytes
///    → `("image/png"` / `"image/jpeg"`, base64_data)`
///
/// Returns an error if none of these match, giving the caller a meaningful
/// message rather than "unexpected shape" deep in the pipeline.
pub fn extract_image_response(response: &str) -> Result<(String, String), String> {
    // 1. SVG
    if let Ok(svg) = extract_svg(response) {
        return Ok(("image/svg+xml".to_string(), svg));
    }

    // 2. Data URL — look for data:image/<subtype>;base64,<b64>
    for mime in &["image/png", "image/jpeg", "image/gif", "image/webp"] {
        let prefix = format!("data:{};base64,", mime);
        if let Some(pos) = response.find(&*prefix) {
            let tail = &response[pos + prefix.len()..];
            // Take until the first character that can't appear in base64 or
            // that terminates a data URL in context (closing paren, quote, backtick, newline).
            let b64: String = tail
                .chars()
                .take_while(|c| {
                    c.is_alphanumeric() || *c == '+' || *c == '/' || *c == '='
                })
                .collect();
            if !b64.is_empty() {
                return Ok((mime.to_string(), b64));
            }
        }
    }

    // 3. Raw base64 identified by decoded magic bytes prefix.
    //    PNG  → \x89PNG\r\n\x1a\n  → base64 starts with "iVBORw0K"
    //    JPEG → \xff\xd8\xff       → base64 starts with "/9j/"
    for (b64_prefix, mime) in &[("iVBORw0K", "image/png"), ("/9j/", "image/jpeg")] {
        if let Some(pos) = response.find(b64_prefix) {
            // Walk back to the start of the base64 token.
            let token_start = response[..pos]
                .rfind(|c: char| !(c.is_alphanumeric() || c == '+' || c == '/' || c == '='))
                .map(|p| p + 1)
                .unwrap_or(0);
            let b64: String = response[token_start..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '+' || *c == '/' || *c == '=')
                .collect();
            // Sanity-check: real images are at least several KiB of base64.
            if b64.len() > 256 {
                return Ok((mime.to_string(), b64));
            }
        }
    }

    let snippet: String = response.chars().take(300).collect();
    eprintln!(
        "[extract_image_response] no image found in response ({} chars total).\n  first 300: {snippet:?}",
        response.len(),
    );
    Err(format!(
        "response did not contain an SVG element or recognizable image data \
        (PNG/JPEG); first 300 chars: {snippet:?}"
    ))
}

/// Convert a stored artifact body to raw PNG bytes for vision calls.
///
/// * SVG  → rasterized via resvg (existing path)
/// * Raster (base64) → decoded directly; JPEG is re-encoded to PNG using the
///   `image` crate if available, otherwise returned as-is after decode since
///   the Claude vision API accepts JPEG too.
pub fn to_png_bytes(body: &str, mime_type: &str) -> Result<Vec<u8>, String> {
    if mime_type == "image/svg+xml" {
        return rasterize_svg_to_png(body);
    }
    use base64::Engine;
    let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&clean)
        .map_err(|e| format!("base64 decode failed: {e}"))
}

/// Aspect ratio (width/height) for a raster image stored as base64.
///
/// Reads PNG dimensions from the IHDR header (bytes 16–23 after the 8-byte
/// signature) without fully decoding the image. Returns 1.0 for non-PNG
/// formats or on any parse failure.
pub fn aspect_ratio_of_raster(base64_data: &str) -> f32 {
    use base64::Engine;
    let clean: String = base64_data.chars().filter(|c| !c.is_whitespace()).collect();
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&clean) else {
        return 1.0;
    };
    // PNG layout: [0..8] signature, [8..12] IHDR length, [12..16] "IHDR",
    //             [16..20] width (big-endian u32), [20..24] height (big-endian u32)
    if bytes.len() >= 24 && bytes[0..8] == *b"\x89PNG\r\n\x1a\n" {
        let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if h > 0 {
            return w as f32 / h as f32;
        }
    }
    1.0
}

/// Unified aspect ratio: dispatches to `aspect_ratio_of` (SVG) or
/// `aspect_ratio_of_raster` based on the artifact's MIME type.
pub fn aspect_ratio_for(body: &str, mime_type: &str) -> f32 {
    if mime_type == "image/svg+xml" {
        aspect_ratio_of(body)
    } else {
        aspect_ratio_of_raster(body)
    }
}

/// Format an artifact body as HTML suitable for setting as `innerHTML` in the
/// progress preview overlay. SVG is returned as-is; raster images are wrapped
/// in a `<img src="data:…">` tag so the browser renders them.
pub fn preview_html(body: &str, mime_type: &str) -> String {
    if mime_type == "image/svg+xml" {
        body.to_string()
    } else {
        format!(
            r#"<img src="data:{mime_type};base64,{body}" style="max-width:100%;max-height:100%;object-fit:contain;"/>"#
        )
    }
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
    /// Raw body: SVG text for `image/svg+xml`, base64-encoded bytes for raster types.
    pub body: String,
    pub mime_type: String,
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
        assert_eq!(p.critique_model, CLAUDE_SONNET_MODEL);
    }

    #[test]
    fn pipeline_medium_has_two_phases_with_critique() {
        let p = pipeline_for(&ImageQuality::Medium);
        assert_eq!(total_iterations(&p), 10);
        assert_eq!(p.steps.len(), 2);
        assert!(!p.steps[0].critique_first);
        assert!(p.steps[1].critique_first);
        assert_eq!(p.critique_model, CLAUDE_SONNET_MODEL);
    }

    #[test]
    fn pipeline_high_has_three_phases_with_haiku_critique() {
        let p = pipeline_for(&ImageQuality::High);
        assert_eq!(total_iterations(&p), 15);
        assert_eq!(p.steps.len(), 3);
        assert_eq!(p.critique_model, CLAUDE_HAIKU_MODEL);
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

    // ── build_direct_gen_messages ────────────────────────────────────────────

    #[test]
    fn direct_gen_prompt_combines_source_context_instruction() {
        let msgs = build_direct_gen_messages("a black hole", "Dense stellar object", Some("dramatic lighting"));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "a black hole, Dense stellar object, dramatic lighting");
    }

    #[test]
    fn direct_gen_prompt_omits_empty_context_and_instruction() {
        let msgs = build_direct_gen_messages("a nebula", "", None);
        assert_eq!(msgs[0].content, "a nebula");
    }

    // ── extract_image_response ───────────────────────────────────────────────

    #[test]
    fn extract_image_response_finds_svg() {
        let r = "Here it is:\n<svg viewBox=\"0 0 10 10\"><circle/></svg>\nDone.";
        let (mime, body) = extract_image_response(r).unwrap();
        assert_eq!(mime, "image/svg+xml");
        assert!(body.starts_with("<svg"));
    }

    #[test]
    fn extract_image_response_finds_data_url_png() {
        // Minimal fake base64 that looks like a data URL
        let r = "Here is the image: data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let (mime, b64) = extract_image_response(r).unwrap();
        assert_eq!(mime, "image/png");
        assert!(!b64.is_empty());
    }

    #[test]
    fn extract_image_response_finds_raw_png_base64() {
        // Build real base64 from a real 1×1 PNG so the magic bytes test works
        use base64::Engine;
        let svg = r#"<svg viewBox="0 0 4 4" xmlns="http://www.w3.org/2000/svg"><rect width="4" height="4" fill="red"/></svg>"#;
        let png = rasterize_svg_to_png(svg).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        // Feed only the raw base64 (no data: prefix)
        let (mime, body) = extract_image_response(&b64).unwrap();
        assert_eq!(mime, "image/png");
        assert!(!body.is_empty());
    }

    #[test]
    fn extract_image_response_errors_on_plain_text() {
        assert!(extract_image_response("just some prose, no image").is_err());
    }

    // ── aspect_ratio_of_raster ───────────────────────────────────────────────

    #[test]
    fn aspect_ratio_of_raster_reads_png_dimensions() {
        use base64::Engine;
        // Produce a real 100×50 PNG so we can parse its header
        let svg = r#"<svg viewBox="0 0 100 50" xmlns="http://www.w3.org/2000/svg"><rect width="100" height="50" fill="blue"/></svg>"#;
        let png = rasterize_svg_to_png(svg).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        // aspect = 512/256 = 2.0 (resvg scales so long edge = 512)
        let ar = aspect_ratio_of_raster(&b64);
        assert!((ar - 2.0).abs() < 0.05, "expected ~2.0, got {ar}");
    }

    #[test]
    fn aspect_ratio_of_raster_returns_one_for_garbage() {
        assert_eq!(aspect_ratio_of_raster("notbase64!!"), 1.0);
    }

    // ── to_png_bytes ─────────────────────────────────────────────────────────

    #[test]
    fn to_png_bytes_roundtrips_raster() {
        use base64::Engine;
        let svg = r#"<svg viewBox="0 0 10 10" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10" fill="green"/></svg>"#;
        let png = rasterize_svg_to_png(svg).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        let back = to_png_bytes(&b64, "image/png").unwrap();
        assert_eq!(back, png);
    }

    #[test]
    fn to_png_bytes_rasterizes_svg() {
        let svg = r#"<svg viewBox="0 0 10 10" xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"#;
        let bytes = to_png_bytes(svg, "image/svg+xml").unwrap();
        // PNG signature
        assert_eq!(&bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
    }
}
