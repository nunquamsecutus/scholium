//! Markdown → annotated HTML renderer.
//!
//! # How source-position annotation works
//!
//! Every Tauri command that returns chapter content now returns fully-rendered
//! HTML rather than raw markdown.  The HTML carries `data-src-*` attributes that
//! let the frontend translate a user's text selection back to exact byte offsets
//! in the reconstructed markdown source, without any text searching.
//!
//! ## Attributes emitted
//!
//! | Attribute | Meaning |
//! |---|---|
//! | `data-src-start="N"` | UTF-8 byte offset in the source where this text begins |
//! | `data-src-end="N"` | UTF-8 byte offset where this text ends (exclusive) |
//! | `data-src-skip` | Element's visible content does NOT correspond to source bytes; the frontend DOM walker skips its entire subtree when building the offset map |
//!
//! `data-src-start` / `data-src-end` are placed on:
//! - An inline `<span>` wrapping each contiguous run of body text (one per
//!   pulldown-cmark `Text` event, split further when custom anchor markers are
//!   found inside the run).
//! - Block-level elements (`<p>`, `<h1>`–`<h6>`, `<li>`, `<blockquote>`) also
//!   receive the span of the whole block; this gives the frontend a coarse anchor
//!   when needed.
//!
//! `data-src-skip` is placed on:
//! - Note-anchor superscripts (`[^*N]`, `[^†N]`, `[^‡N]`) — the marker bytes
//!   are consumed by the anchor and do not appear in selectable text.
//! - Appendix cross-reference links (`[^AN]`).
//! - `<figure>` elements wrapping inlined SVG artifacts — the image is not body
//!   text.
//! - The rendered endnote section appended at the end of the chapter.
//!
//! ## Frontend contract
//!
//! When the user makes a text selection the frontend:
//! 1. Walks up from `range.startContainer` to find the nearest ancestor with
//!    `data-src-start`, skipping any subtree rooted at a `data-src-skip` element.
//! 2. Computes `srcStart = spanSrcStart + TextEncoder.encode(prefix).length`
//!    where `prefix` is the span's text content up to the selection start offset.
//!    (`TextEncoder` produces UTF-8 bytes, matching Rust's string indexing.)
//! 3. Does the same from `range.endContainer` to get `srcEnd`.
//! 4. Sends `{ srcStart, srcEnd, selectionText }` to the backend command.
//!
//! The backend then operates at exact byte positions — no text searching needed.
//!
//! ## SVG sanitization
//!
//! Artifact SVG bodies are sanitized with `ammonia` when inlined.  This replaces
//! the previous approach of running DOMPurify in the frontend after rendering.

use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd};
use crate::edupage::{ArtifactWithBody, NoteWithBody};

/// Regex-like scan for the custom note-anchor patterns that live in the body
/// text after `edupage::reconstruct`.  These are not standard Markdown so
/// pulldown-cmark surfaces them as plain `Text` events; we split them out here.
///
/// Patterns:
/// - `[^*N]`  definition  (asterisk)
/// - `[^†N]`  footnote    (dagger)
/// - `[^‡N]`  endnote     (double-dagger)
/// - `[^AN]`  appendix ref
///
/// Returns a list of segments: `(text_slice, is_anchor)`.
fn split_anchors(s: &str) -> Vec<(&str, bool)> {
    let mut segments: Vec<(&str, bool)> = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        // Find the earliest '[^' that starts an anchor.
        if let Some(open) = rest.find("[^") {
            // Check whether the character after '[^' is one of our markers.
            let after = &rest[open + 2..];
            let marker_len = if after.starts_with('*')
                || after.starts_with('†')
                || after.starts_with('‡')
                || after.starts_with('A')
            {
                after.chars().next().map(|c| c.len_utf8()).unwrap_or(0)
            } else {
                0
            };
            if marker_len > 0 {
                // Scan forward for the closing ']'.
                let inner_start = open + 2 + marker_len;
                if let Some(rel_close) = rest[inner_start..].find(']') {
                    let close = inner_start + rel_close;
                    // Everything before '[^…' is plain text.
                    if open > 0 {
                        segments.push((&rest[..open], false));
                    }
                    segments.push((&rest[open..=close], true));
                    rest = &rest[close + 1..];
                    continue;
                }
            }
            // Not an anchor — advance past this '[' and keep looking.
            segments.push((&rest[..open + 1], false));
            rest = &rest[open + 1..];
        } else {
            segments.push((rest, false));
            break;
        }
    }
    segments
}

/// Parse a note-anchor string like `[^*3]` and return `(marker_char, id)`.
fn parse_anchor(anchor: &str) -> Option<(char, u32)> {
    // anchor = "[^<marker><digits>]"
    let inner = anchor.strip_prefix("[^")?.strip_suffix(']')?;
    let marker = inner.chars().next()?;
    let id_str = &inner[marker.len_utf8()..];
    let id = id_str.parse::<u32>().ok()?;
    Some((marker, id))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Emit annotated inline text — potentially split around anchor markers.
///
/// `src_start` is the byte offset of `text` inside the full reconstructed
/// markdown.  For each plain sub-segment a `<span data-src-start … >` is
/// emitted; for each anchor a `<sup data-src-skip …>` (or `<a data-src-skip
/// …>` for appendix refs) is emitted.
fn emit_text(
    out: &mut String,
    text: &str,
    src_start: usize,
    notes: &[NoteWithBody],
) {
    let segments = split_anchors(text);
    let mut byte_cursor = src_start;

    for (seg, is_anchor) in segments {
        let seg_bytes = seg.len(); // UTF-8 byte length

        if is_anchor {
            // Consume the anchor bytes from the source offset map but emit a
            // skip element — the display text has no correspondence to source.
            if let Some((marker, id)) = parse_anchor(seg) {
                match marker {
                    'A' => {
                        out.push_str(&format!(
                            r#"<a data-src-skip class="appendix-ref" data-appendix-seq="{id}" role="link" tabindex="0">(see Appendix {id})</a>"#
                        ));
                    }
                    _ => {
                        let note_type = match marker {
                            '*' => "definition",
                            '†' => "footnote",
                            '‡' => "endnote",
                            _ => "unknown",
                        };
                        let display = match marker {
                            '*' => "📖".to_string(),
                            _ => id.to_string(),
                        };
                        let note = notes.iter().find(|n| n.id == id);
                        let aria = match note {
                            Some(n) if note_type == "definition" => {
                                format!("{note_type} of {}", html_escape(&n.word))
                            }
                            Some(_) => format!("{} {}", title_case(note_type), id),
                            None => format!("note {id}"),
                        };
                        out.push_str(&format!(
                            r#"<sup data-src-skip class="note-anchor" data-note-id="{id}" data-note-type="{note_type}" aria-label="{aria}">{display}</sup>"#
                        ));
                    }
                }
            }
        } else {
            let seg_src_end = byte_cursor + seg_bytes;
            out.push_str(&format!(
                r#"<span data-src-start="{byte_cursor}" data-src-end="{seg_src_end}">{}</span>"#,
                html_escape(seg)
            ));
        }

        byte_cursor += seg_bytes;
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Sanitize an SVG string for safe inline rendering.
///
/// `ammonia`'s defaults cover common HTML elements but do **not** include SVG
/// tags (`<svg>`, `<rect>`, `<path>`, …); without configuration every SVG tag
/// is stripped and the figure renders empty.  This builder extends the default
/// allowlist with the standard SVG element set and safe SVG attributes, while
/// still blocking `<script>` tags and `on*` event-handler attributes.
fn clean_svg(svg: &str) -> String {
    let mut b = ammonia::Builder::new();
    b.add_tags([
        // structural
        "svg", "g", "defs", "title", "desc", "symbol", "use",
        // shapes
        "path", "rect", "circle", "ellipse",
        "line", "polyline", "polygon",
        // text
        "text", "tspan", "textPath",
        // gradients / patterns / clipping
        "linearGradient", "radialGradient", "stop",
        "clipPath", "mask", "pattern", "marker",
        // filters (allow the container; primitives below)
        "filter",
        "feBlend", "feColorMatrix", "feComponentTransfer",
        "feComposite", "feFlood", "feGaussianBlur",
        "feMerge", "feMergeNode", "feMorphology", "feOffset",
        "feTile", "feTurbulence",
        // images / animation
        "image",
        "animate", "animateTransform", "animateMotion",
    ]);
    b.add_generic_attributes([
        "id", "class", "style",
        // geometry
        "x", "y", "width", "height",
        "cx", "cy", "r", "rx", "ry",
        "x1", "y1", "x2", "y2",
        "points", "d",
        "viewBox", "preserveAspectRatio",
        // presentation
        "fill", "fill-opacity", "fill-rule",
        "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin",
        "stroke-dasharray", "stroke-dashoffset", "stroke-opacity", "stroke-miterlimit",
        "opacity", "display", "visibility",
        "transform",
        "font-family", "font-size", "font-weight", "font-style",
        "text-anchor", "dominant-baseline", "alignment-baseline",
        "letter-spacing", "word-spacing",
        // gradient / stop
        "offset", "stop-color", "stop-opacity",
        "gradientUnits", "gradientTransform", "spreadMethod",
        "patternUnits", "patternTransform",
        "fx", "fy",
        // xmlns (on the root <svg>)
        "xmlns",
        // use / symbol links  (no javascript: — ammonia blocks unlisted schemes)
        "href",
        // marker geometry
        "markerWidth", "markerHeight", "markerUnits", "orient", "refX", "refY",
        // clip / mask references
        "clip-path", "clip-rule",
        // filter primitives
        "in", "in2", "result", "type", "mode", "values",
        "stdDeviation", "dx", "dy",
    ]);
    b.clean(svg).to_string()
}

/// Inline an artifact as a `<figure data-src-skip …>`.
///
/// The figure carries `data-src-skip` because the image is not selectable body
/// text.
///
/// * **SVG** artifacts: the body is sanitized with `clean_svg` (allows all
///   standard SVG elements, strips `<script>` and `on*` handlers) and inlined
///   directly into the HTML.
/// * **Raster** artifacts (`image/png`, `image/jpeg`, …): the base64 body is
///   rendered as an `<img src="data:…">` tag so no external file is needed.
fn figure_for(artifact: &ArtifactWithBody) -> String {
    let cls = if artifact.aspect_ratio >= 1.0 {
        "artifact artifact-block"
    } else {
        "artifact artifact-float"
    };
    let content = if artifact.mime_type == "image/svg+xml" {
        clean_svg(&artifact.body)
    } else {
        let alt = html_escape(artifact.caption.as_deref().unwrap_or(""));
        format!(
            r#"<img src="data:{};base64,{}" alt="{}" style="max-width:100%;height:auto;"/>"#,
            artifact.mime_type, artifact.body, alt,
        )
    };
    let caption = artifact
        .caption
        .as_deref()
        .map(|c| format!("<figcaption>{}</figcaption>", html_escape(c)))
        .unwrap_or_default();
    format!(
        r#"<figure data-src-skip class="{cls}" data-artifact-id="{id}">{content}{caption}</figure>"#,
        id = artifact.id,
    )
}

/// Merge consecutive `Text` events that are contiguous in source bytes.
///
/// pulldown-cmark splits text on characters that might start inline markup
/// (e.g. `[`, `*`).  For the markdown `"word[^*1] rest"` it emits:
///   Text("word"), Text("["), Text("^"), Text("*"), Text("1"), Text("]"), Text(" rest")
///
/// Our `split_anchors` helper only recognises the full `[^*N]` pattern.  By
/// merging adjacent text events that sit at contiguous byte positions we
/// reconstruct the original run so `split_anchors` can find the markers.
fn merge_adjacent_texts(events: Vec<(Event<'_>, std::ops::Range<usize>)>) -> Vec<(Event<'_>, std::ops::Range<usize>)> {
    let mut result: Vec<(Event<'_>, std::ops::Range<usize>)> = Vec::with_capacity(events.len());
    let mut i = 0;
    while i < events.len() {
        if let (Event::Text(t), range) = &events[i] {
            // Accumulate all immediately-following Text events at contiguous offsets.
            let mut combined: String = t.as_ref().to_owned();
            let start = range.start;
            let mut end = range.end;
            let mut j = i + 1;
            while j < events.len() {
                if let (Event::Text(t2), r2) = &events[j] {
                    if r2.start == end {
                        combined.push_str(t2.as_ref());
                        end = r2.end;
                        j += 1;
                        continue;
                    }
                }
                break;
            }
            result.push((Event::Text(CowStr::from(combined)), start..end));
            i = j;
        } else {
            // Non-Text events: clone and forward.
            // SAFETY: Event<'_> borrows from `markdown` which outlives this Vec.
            #[allow(clippy::clone_on_copy)]
            result.push(events[i].clone());
            i += 1;
        }
    }
    result
}

/// Render an edupage's reconstructed markdown to annotated HTML.
///
/// `markdown` must be the *reconstructed* markdown (the output of
/// `edupage::reconstruct`), not the raw `.edupage` file.  The byte offsets
/// emitted in `data-src-start` / `data-src-end` attributes refer to positions
/// in this string.
pub fn render_chapter_html(
    markdown: &str,
    notes: &[NoteWithBody],
    artifacts: &[ArtifactWithBody],
) -> String {
    let artifact_map: std::collections::HashMap<&str, &ArtifactWithBody> =
        artifacts.iter().map(|a| (a.id.as_str(), a)).collect();

    let opts = Options::empty();

    // Collect events with their byte ranges so we know where each piece of
    // source text lives.  Merge adjacent Text events first — pulldown-cmark
    // splits on potential-link characters like '[', which would prevent
    // split_anchors from seeing our custom [^marker…] patterns whole.
    let events: Vec<(Event<'_>, std::ops::Range<usize>)> =
        merge_adjacent_texts(
            Parser::new_ext(markdown, opts).into_offset_iter().collect()
        );

    let mut out = String::with_capacity(markdown.len() * 3);

    // Track block-level src range for annotation.
    let mut block_src_start: usize = 0;
    let mut block_src_end: usize = 0;
    // Whether we are currently inside a block whose open tag we've already
    // emitted and need to close.
    let mut open_block_tag: Option<&'static str> = None;

    // We emit opening block tags *after* we know the block's source range, so
    // we buffer just the opening tag string.
    let mut pending_block_open: Option<String> = None;

    for (event, range) in &events {
        match event {
            // ── Block-level opens ────────────────────────────────────────────
            Event::Start(Tag::Paragraph) => {
                block_src_start = range.start;
                block_src_end = range.end;
                pending_block_open = Some(format!(
                    r#"<p data-src-start="{}" data-src-end="{}">"#,
                    range.start, range.end
                ));
                open_block_tag = Some("p");
            }
            Event::Start(Tag::Heading { level, .. }) => {
                block_src_start = range.start;
                block_src_end = range.end;
                let tag = heading_tag(*level);
                pending_block_open = Some(format!(
                    r#"<{tag} data-src-start="{}" data-src-end="{}">"#,
                    range.start, range.end
                ));
                open_block_tag = Some(tag);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                out.push_str(&format!(
                    r#"<blockquote data-src-start="{}" data-src-end="{}">"#,
                    range.start, range.end
                ));
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                out.push_str("</blockquote>");
            }
            Event::Start(Tag::List(Some(_))) => out.push_str("<ol>"),
            Event::Start(Tag::List(None)) => out.push_str("<ul>"),
            Event::End(TagEnd::List(true)) => out.push_str("</ol>"),
            Event::End(TagEnd::List(false)) => out.push_str("</ul>"),
            Event::Start(Tag::Item) => {
                out.push_str(&format!(
                    r#"<li data-src-start="{}" data-src-end="{}">"#,
                    range.start, range.end
                ));
            }
            Event::End(TagEnd::Item) => out.push_str("</li>"),

            // ── Block-level closes ───────────────────────────────────────────
            Event::End(TagEnd::Paragraph) => {
                out.push_str("</p>");
                open_block_tag = None;
            }
            Event::End(TagEnd::Heading(level)) => {
                out.push_str(&format!("</{}>", heading_tag(*level)));
                open_block_tag = None;
            }

            // ── Inline formatting ────────────────────────────────────────────
            Event::Start(Tag::Emphasis) => out.push_str("<em>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</em>"),
            Event::Start(Tag::Strong) => out.push_str("<strong>"),
            Event::End(TagEnd::Strong) => out.push_str("</strong>"),
            Event::Start(Tag::Strikethrough) => out.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => out.push_str("</s>"),
            Event::Start(Tag::Link { dest_url, title, .. }) => {
                let title_attr = if title.is_empty() {
                    String::new()
                } else {
                    format!(r#" title="{}""#, html_escape(title))
                };
                out.push_str(&format!(
                    r#"<a href="{}"{}>"#,
                    html_escape(dest_url),
                    title_attr
                ));
            }
            Event::End(TagEnd::Link) => out.push_str("</a>"),
            Event::Start(Tag::Image { dest_url, .. }) => {
                // epar:// images are artifact references — inline the SVG.
                if let Some(id_str) = dest_url.strip_prefix("epar://") {
                    if let Some(art) = artifact_map.get(id_str) {
                        // Flush any pending block open tag first.
                        if let Some(open) = pending_block_open.take() {
                            out.push_str(&open);
                        }
                        out.push_str(&figure_for(art));
                    }
                }
                // Other images fall through; End(Image) closes them.
            }
            Event::End(TagEnd::Image) => {
                // Nothing extra needed — artifact figures closed in Start above.
            }

            // ── Inline code ──────────────────────────────────────────────────
            Event::Code(s) => {
                // Inline code: the content is a verbatim text run. Emit with
                // src annotation covering the backtick-delimited source span.
                // pulldown-cmark includes the backticks in the range; the
                // displayed text is just the inner content, but we annotate
                // the full source span so the backend can locate the code block
                // precisely.
                out.push_str("<code>");
                out.push_str(&format!(
                    r#"<span data-src-start="{}" data-src-end="{}">{}</span>"#,
                    range.start,
                    range.end,
                    html_escape(s)
                ));
                out.push_str("</code>");
            }

            // ── Code blocks ──────────────────────────────────────────────────
            Event::Start(Tag::CodeBlock(_)) => {
                out.push_str("<pre><code>");
            }
            Event::End(TagEnd::CodeBlock) => {
                out.push_str("</code></pre>");
            }

            // ── Inline HTML (rewrite spans, etc.) ────────────────────────────
            Event::Html(raw) | Event::InlineHtml(raw) => {
                // We trust Rust-generated HTML that has been previously stored
                // in the edupage file (rewrite spans).  Pass it through as-is.
                // The inner text of a rewrite span was inserted verbatim into
                // the source at a known byte position, so existing
                // data-src-start / data-src-end attributes (if any) remain
                // valid.  Raw LLM HTML that does not carry those attributes is
                // harmless — the frontend walker simply finds no annotation
                // and treats it as a skip zone.
                out.push_str(raw);
            }

            // ── Text ─────────────────────────────────────────────────────────
            Event::Text(s) => {
                // Flush the pending block open tag now that we know we have
                // content for it.
                if let Some(open) = pending_block_open.take() {
                    out.push_str(&open);
                }
                emit_text(&mut out, s, range.start, notes);
            }

            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str("<hr>"),

            _ => {}
        }

        // Keep block_src_end up to date as we advance through the block.
        if range.end > block_src_end {
            block_src_end = range.end;
        }
        let _ = (block_src_start, open_block_tag); // suppress unused warnings
    }

    // Append the endnote section, marked data-src-skip throughout since it is
    // rendered margin content and not selectable body text.
    let endnotes: Vec<&NoteWithBody> = notes.iter().filter(|n| n.note_type_str() == "endnote").collect();
    if !endnotes.is_empty() {
        out.push_str(r#"<hr class="endnotes-rule"><section data-src-skip class="endnotes" aria-label="Endnotes"><h2 class="endnotes-heading">Endnotes</h2><ol class="endnotes-list">"#);
        for en in endnotes {
            let body_html = render_note_body(&en.body);
            out.push_str(&format!(
                r#"<li id="endnote-{id}" data-endnote-target="{id}" class="endnote-item"><span class="endnote-number">{id}.</span><div class="endnote-body">{body_html}</div></li>"#,
                id = en.id,
            ));
        }
        out.push_str("</ol></section>");
    }

    out
}

/// Render a note body (footnote / endnote body text) to plain HTML.
///
/// Note bodies are displayed in margin or bottom-sheet UI — they don't need
/// source-position annotation.  We run a plain pulldown-cmark pass with HTML
/// escaping but no `data-src-*` attributes.
fn render_note_body(markdown: &str) -> String {
    use pulldown_cmark::html;
    let parser = Parser::new(markdown);
    let mut html_out = String::new();
    html::push_html(&mut html_out, parser);
    html_out
}

fn heading_tag(level: pulldown_cmark::HeadingLevel) -> &'static str {
    use pulldown_cmark::HeadingLevel::*;
    match level {
        H1 => "h1",
        H2 => "h2",
        H3 => "h3",
        H4 => "h4",
        H5 => "h5",
        H6 => "h6",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_notes() -> Vec<NoteWithBody> {
        vec![]
    }
    fn no_artifacts() -> Vec<ArtifactWithBody> {
        vec![]
    }

    // ── split_anchors ────────────────────────────────────────────────────────

    #[test]
    fn split_anchors_plain_text() {
        let segs = split_anchors("hello world");
        assert_eq!(segs, vec![("hello world", false)]);
    }

    #[test]
    fn split_anchors_single_definition_anchor() {
        let segs = split_anchors("blackhole[^*1] here");
        assert_eq!(
            segs,
            vec![("blackhole", false), ("[^*1]", true), (" here", false)]
        );
    }

    #[test]
    fn split_anchors_footnote_dagger() {
        let segs = split_anchors("text[^†2]more");
        assert_eq!(
            segs,
            vec![("text", false), ("[^†2]", true), ("more", false)]
        );
    }

    #[test]
    fn split_anchors_appendix_ref() {
        let segs = split_anchors("see[^A3] appendix");
        assert_eq!(
            segs,
            vec![("see", false), ("[^A3]", true), (" appendix", false)]
        );
    }

    #[test]
    fn split_anchors_multiple() {
        let segs = split_anchors("[^*1]middle[^†2]");
        assert_eq!(
            segs,
            vec![("[^*1]", true), ("middle", false), ("[^†2]", true)]
        );
    }

    // ── data-src-start / data-src-end correctness ────────────────────────────

    #[test]
    fn plain_paragraph_has_src_annotations() {
        let html = render_chapter_html("Hello world.", &no_notes(), &no_artifacts());
        // The text "Hello world." starts at byte 0 in the source.
        assert!(html.contains(r#"data-src-start="0""#));
        assert!(html.contains("Hello world."));
    }

    #[test]
    fn note_anchor_gets_data_src_skip() {
        let md = "The blackhole[^*1] is here.";
        let html = render_chapter_html(md, &no_notes(), &no_artifacts());
        assert!(html.contains("data-src-skip"));
        assert!(html.contains(r#"data-note-id="1""#));
        // The text before and after the anchor must still be annotated.
        assert!(html.contains("The blackhole"));
        assert!(html.contains(" is here."));
    }

    #[test]
    fn appendix_ref_gets_data_src_skip() {
        let md = "See[^A2] the appendix.";
        let html = render_chapter_html(md, &no_notes(), &no_artifacts());
        assert!(html.contains("data-src-skip"));
        assert!(html.contains(r#"data-appendix-seq="2""#));
    }

    #[test]
    fn src_offsets_are_correct_for_text_after_anchor() {
        // "The blackhole[^*1] is here."
        //  0123456789012345678901234567
        //  "The blackhole" = bytes 0–12
        //  "[^*1]"         = bytes 13–17
        //  " is here."     = bytes 18–27
        let md = "The blackhole[^*1] is here.";
        let html = render_chapter_html(md, &no_notes(), &no_artifacts());
        // The segment after the anchor starts at byte 18.
        assert!(html.contains(r#"data-src-start="18""#), "html={html}");
    }

    #[test]
    fn heading_has_src_annotations() {
        let md = "# Title\n\nBody.";
        let html = render_chapter_html(md, &no_notes(), &no_artifacts());
        assert!(html.contains("<h1"));
        assert!(html.contains("data-src-start="));
        assert!(html.contains("Title"));
    }

    // ── SVG sanitization ─────────────────────────────────────────────────────

    #[test]
    fn clean_svg_passes_through_standard_svg() {
        let svg = r#"<svg viewBox="0 0 100 50"><rect width="100" height="50" fill="blue"/></svg>"#;
        let out = clean_svg(svg);
        assert!(out.contains("<svg"), "svg tag should survive: {out}");
        assert!(out.contains("<rect"), "rect tag should survive: {out}");
        assert!(out.contains("fill=\"blue\""), "fill attr should survive: {out}");
    }

    #[test]
    fn clean_svg_strips_script_tags() {
        let svg = r#"<svg><script>alert(1)</script><rect width="10" height="10"/></svg>"#;
        let out = clean_svg(svg);
        assert!(!out.contains("<script"), "script must be stripped: {out}");
        assert!(out.contains("<rect"), "rect should survive: {out}");
    }

    #[test]
    fn clean_svg_strips_event_handlers() {
        let svg = r#"<svg><rect onclick="evil()" width="10" height="10"/></svg>"#;
        let out = clean_svg(svg);
        assert!(!out.contains("onclick"), "onclick must be stripped: {out}");
        assert!(out.contains("<rect"), "rect should survive: {out}");
    }

    #[test]
    fn artifact_figure_contains_svg() {
        use crate::edupage::ArtifactWithBody;
        let art = ArtifactWithBody {
            id: "abc123def456.svg".to_string(),
            mime_type: "image/svg+xml".into(),
            semantic_type: "diagram".into(),
            ctime: "2026-01-01T00:00:00Z".into(),
            caption: Some("Test diagram".into()),
            aspect_ratio: 1.5,
            source: "the collapse".into(),
            body: r#"<svg viewBox="0 0 100 50"><circle cx="50" cy="25" r="20" fill="red"/></svg>"#.into(),
        };
        let html = figure_for(&art);
        assert!(html.contains("<svg"), "figure must contain <svg>: {html}");
        assert!(html.contains("<circle"), "figure must contain <circle>: {html}");
        assert!(html.contains("Test diagram"), "figure must contain caption: {html}");
        assert!(html.contains("data-src-skip"), "figure must carry data-src-skip: {html}");
    }

    #[test]
    fn artifact_figure_renders_raster_as_img_tag() {
        use crate::edupage::ArtifactWithBody;
        let art = ArtifactWithBody {
            id: "def789abc012.png".to_string(),
            mime_type: "image/png".into(),
            semantic_type: "image".into(),
            ctime: "2026-01-01T00:00:00Z".into(),
            caption: Some("A photo".into()),
            aspect_ratio: 1.78,
            source: "the star".into(),
            body: "iVBORfakebase64==".into(),
        };
        let html = figure_for(&art);
        assert!(html.contains(r#"src="data:image/png;base64,iVBORfakebase64=="#), "img src must be a data URL: {html}");
        assert!(html.contains("<img"), "must use img tag for raster: {html}");
        assert!(!html.contains("<svg"), "must not emit svg tag for raster: {html}");
        assert!(html.contains("A photo"), "caption must appear: {html}");
        assert!(html.contains("data-src-skip"), "figure must carry data-src-skip: {html}");
    }
}

