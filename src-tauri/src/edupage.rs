//! Chapter file format: JSON frontmatter + markdown body + footnote definitions.
//!
//! # File format
//!
//! ```text
//! ---
//! {"nextNoteId":2,"notes":[{"id":1,"type":"definition","word":"blackhole","ctime":"..."}]}
//! ---
//!
//! # Chapter Title
//!
//! Content here. Blackhole[^*1] is a fascinating object.
//!
//! [^*1]: A region of spacetime where gravity is so strong that nothing can escape.
//! ```
//!
//! Footnote definition lines follow `[^<marker><id>]: body`.
//! Multi-line bodies use 4-space continuation.  The `[^...]` anchor syntax is
//! **not** standard CommonMark; pulldown-cmark (used without the `footnotes`
//! feature flag) renders these as plain text.  `read()` strips definition lines
//! before passing content to the renderer.
//!
//! # Artifact references
//!
//! Artifacts (images, diagrams) are stored as `![alt](epar://sha1.ext)` in the
//! markdown body.  The SHA1 is content-addressed: `sha1(file_bytes).ext`.
//! Artifact metadata lives in the book manifest (`Manifest.artifacts`), not in
//! the chapter file.

use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────────

fn default_next_note_id() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChapterFrontmatter {
    #[serde(default = "default_next_note_id")]
    pub next_note_id: u32,
    #[serde(default)]
    pub notes: Vec<NoteMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteType {
    #[serde(rename = "definition")]
    Definition,
    #[serde(rename = "footnote")]
    Footnote,
    #[serde(rename = "endnote")]
    Endnote,
}

/// The marker character that follows `[^` in the source-level anchor.
/// Asterisk for definitions, dagger for footnotes, double-dagger for endnotes.
pub fn marker_char(note_type: &NoteType) -> char {
    match note_type {
        NoteType::Definition => '*',
        NoteType::Footnote => '†',
        NoteType::Endnote => '‡',
    }
}

fn anchor_text(note_type: &NoteType, id: u32) -> String {
    format!("[^{}{}]", marker_char(note_type), id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    pub id: u32,
    #[serde(rename = "type")]
    pub note_type: NoteType,
    pub word: String,
    pub ctime: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWithBody {
    pub id: u32,
    #[serde(rename = "type")]
    pub note_type: NoteType,
    pub word: String,
    pub ctime: String,
    pub body: String,
}

impl NoteWithBody {
    pub fn note_type_str(&self) -> &'static str {
        match self.note_type {
            NoteType::Definition => "definition",
            NoteType::Footnote => "footnote",
            NoteType::Endnote => "endnote",
        }
    }
}

/// Artifact metadata stored at the manifest level.
///
/// `id` is `"<sha1hex>.<ext>"` (content-addressed).  The file on disk lives
/// at `images/<id>` inside the book directory.  In chapter markdown, the
/// artifact is referenced as `![alt](epar://<id>)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMeta {
    /// Content-addressed ID: `sha1hex.ext`, e.g., `"abc123def.svg"`.
    pub id: String,
    pub mime_type: String,
    /// "image" | "diagram" etc.
    pub semantic_type: String,
    pub ctime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// width / height from the SVG viewBox. Drives block vs float layout.
    pub aspect_ratio: f32,
    /// The text the artifact was generated from (for re-generation).
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactWithBody {
    pub id: String,
    pub mime_type: String,
    pub semantic_type: String,
    pub ctime: String,
    pub caption: Option<String>,
    pub aspect_ratio: f32,
    pub source: String,
    pub body: String,
}

/// Parsed chapter: markdown content (footnote defs stripped) and notes with
/// their bodies.  Artifact metadata lives in the manifest, not here.
#[derive(Debug, Serialize)]
pub struct EduPage {
    pub content: String,
    pub notes: Vec<NoteWithBody>,
}

// ── Private format helpers ────────────────────────────────────────────────────

/// Parse frontmatter from a raw chapter file.
/// Returns `(frontmatter, body_including_footnote_defs)`.
fn parse_raw(raw: &str) -> Result<(ChapterFrontmatter, String), String> {
    let raw = raw.trim_start();
    let rest = raw
        .strip_prefix("---\n")
        .ok_or("missing frontmatter opening ---")?;
    let (json, rest) = rest
        .split_once("\n---\n")
        .ok_or("missing frontmatter closing ---")?;
    let fm: ChapterFrontmatter =
        serde_json::from_str(json).map_err(|e| format!("invalid chapter frontmatter: {e}"))?;
    // Strip leading blank line after the closing `---`.
    let body = rest.trim_start_matches('\n').to_string();
    Ok((fm, body))
}

/// Serialize frontmatter + content + footnote defs back to raw file bytes.
fn write_raw(fm: &ChapterFrontmatter, content: &str, notes: &[NoteWithBody]) -> String {
    let json = serde_json::to_string(fm).expect("serialize frontmatter");
    let mut body = content.trim_end().to_string();

    let note_defs: Vec<NoteWithBody> = {
        // Write in note-id order.
        let mut ordered = notes.to_vec();
        ordered.sort_by_key(|n| n.id);
        ordered
    };

    if !note_defs.is_empty() {
        body.push_str("\n\n");
        for note in &note_defs {
            body.push_str(&format_footnote_def(note));
            body.push('\n');
        }
    }

    format!("---\n{}\n---\n\n{}", json, body)
}

/// Format a footnote definition block for a note.
///
/// Single-line bodies: `[^*1]: body`
/// Multi-line bodies: first line as above, continuation with 4-space indent.
fn format_footnote_def(note: &NoteWithBody) -> String {
    let marker = marker_char(&note.note_type);
    let mut lines = note.body.lines();
    let first = lines.next().unwrap_or("");
    let mut result = format!("[^{}{}]: {}", marker, note.id, first);
    for line in lines {
        result.push('\n');
        result.push_str("    ");
        result.push_str(line);
    }
    result
}

/// Parse a footnote definition header line.
/// Returns `(marker_char, note_id, body_text)` if the line matches.
fn parse_footnote_def_header(line: &str) -> Option<(char, u32, &str)> {
    let rest = line.strip_prefix("[^")?;
    let marker = rest.chars().next()?;
    if !matches!(marker, '*' | '†' | '‡') {
        return None;
    }
    let after_marker = &rest[marker.len_utf8()..];
    // Expect `<digits>]: ` or `<digits>]:` at end-of-line
    let colon_bracket = after_marker.find("]:")?;
    let id_str = &after_marker[..colon_bracket];
    if !id_str.chars().all(|c| c.is_ascii_digit()) || id_str.is_empty() {
        return None;
    }
    let id: u32 = id_str.parse().ok()?;
    let body_start = colon_bracket + 2; // skip "]:"
    let body = after_marker[body_start..].trim_start_matches(' ');
    Some((marker, id, body))
}

/// Split a body string (after frontmatter) into:
/// - `content`: the markdown without footnote definitions
/// - `defs`: `(marker_char, note_id, body_text)` for each def found at the end
///
/// Scans bottom-up so we can identify the footnote section without knowing
/// its extent in advance.  Because continuation lines (`    body`) appear
/// *after* their header in the file they are encountered *before* the header
/// when scanning backwards; we buffer them and attach them when the header
/// is found.  If buffered continuations are never claimed by a header (i.e. we
/// hit blank lines or regular content before finding one) they are returned to
/// the content section.
fn extract_footnote_defs(body: &str) -> (String, Vec<(char, u32, String)>) {
    let lines: Vec<&str> = body.lines().collect();
    let mut defs: Vec<(char, u32, String)> = Vec::new();
    let mut current: Option<(char, u32, String)> = None;
    // Continuations encountered bottom-up before their header is found.
    let mut pending: Vec<String> = Vec::new();
    let mut i = lines.len();

    // Skip trailing blank lines.
    while i > 0 && lines[i - 1].trim().is_empty() {
        i -= 1;
    }

    // Scan bottom-up collecting def lines.
    while i > 0 {
        let line = lines[i - 1];
        if let Some(stripped) = line.strip_prefix("    ") {
            // Continuation line — buffer it for the header we'll see next.
            pending.insert(0, stripped.to_string());
            i -= 1;
        } else if let Some((marker, id, first_line)) = parse_footnote_def_header(line) {
            if let Some(prev) = current.take() {
                defs.push(prev);
            }
            // Attach buffered continuation lines (already in top-to-bottom order).
            let mut body_text = first_line.to_string();
            for cont in &pending {
                body_text.push('\n');
                body_text.push_str(cont);
            }
            pending.clear();
            current = Some((marker, id, body_text));
            i -= 1;
        } else if line.trim().is_empty() {
            if pending.is_empty() {
                // Blank lines between defs are allowed.
                i -= 1;
            } else {
                // Blank line with unclaimed continuations — not a def section.
                i += pending.len();
                pending.clear();
                break;
            }
        } else {
            // Regular content line.
            if !pending.is_empty() {
                // Restore continuation lines back into content.
                i += pending.len();
                pending.clear();
            }
            break;
        }
    }
    if let Some(d) = current {
        defs.push(d);
    }
    defs.reverse(); // restore original (top-to-bottom) order

    // Content is everything up to the start of the def section.
    let content_end = {
        let mut e = i;
        while e > 0 && lines[e - 1].trim().is_empty() {
            e -= 1;
        }
        e
    };
    let content = lines[..content_end].join("\n");
    (content, defs)
}

/// Rebuild NoteWithBody list from metadata + parsed def list.
fn rebuild_notes_with_bodies(
    notes: &[NoteMeta],
    defs: &[(char, u32, String)],
) -> Vec<NoteWithBody> {
    notes
        .iter()
        .map(|meta| {
            let marker = marker_char(&meta.note_type);
            let body = defs
                .iter()
                .find(|(m, id, _)| *m == marker && *id == meta.id)
                .map(|(_, _, b)| b.clone())
                .unwrap_or_default();
            NoteWithBody {
                id: meta.id,
                note_type: meta.note_type.clone(),
                word: meta.word.clone(),
                ctime: meta.ctime.clone(),
                body,
            }
        })
        .collect()
}

// ── Byte-position helpers ─────────────────────────────────────────────────────

/// Return `(line_idx, line_start_byte, line_end_byte)` for the line containing
/// `byte_pos`.  `line_end` is the byte offset of the `\n` separator (or
/// `content.len()` for the last line).
fn locate_byte(content: &str, byte_pos: usize) -> Result<(usize, usize, usize), String> {
    if byte_pos > content.len() {
        return Err(format!(
            "byte position {} exceeds source length {}",
            byte_pos,
            content.len()
        ));
    }
    let mut line_idx = 0;
    let mut line_start = 0;
    for (i, c) in content.char_indices() {
        if i >= byte_pos {
            break;
        }
        if c == '\n' {
            line_idx += 1;
            line_start = i + 1;
        }
    }
    let line_end = content[line_start..]
        .find('\n')
        .map(|r| line_start + r)
        .unwrap_or(content.len());
    Ok((line_idx, line_start, line_end))
}

/// Insert `text` at `byte_pos` in `content`.
fn insert_at_byte(content: &str, byte_pos: usize, text: &str) -> Result<String, String> {
    if byte_pos > content.len() {
        return Err(format!(
            "byte position {} exceeds content length {}",
            byte_pos,
            content.len()
        ));
    }
    if !content.is_char_boundary(byte_pos) {
        return Err(format!(
            "byte position {} is not on a UTF-8 character boundary",
            byte_pos
        ));
    }
    Ok(format!(
        "{}{}{}",
        &content[..byte_pos],
        text,
        &content[byte_pos..]
    ))
}

/// Replace the content of line `line_idx` (the bytes `line_start..line_end`)
/// with `new_text`, preserving the `\n` separator after the line if any.
fn replace_line_content(
    content: &str,
    line_start: usize,
    line_end: usize,
    new_text: &str,
) -> String {
    // content[line_end..] starts at the '\n' (or is empty for last line).
    format!(
        "{}{}{}",
        &content[..line_start],
        new_text,
        &content[line_end..]
    )
}

/// Replace line at `line_idx` (0-based) in a `lines().collect()` sense.
fn replace_line_by_idx(lines: &mut [String], line_idx: usize, new_text: &str) {
    if line_idx < lines.len() {
        lines[line_idx] = new_text.to_string();
    }
}

/// HTML-escape a string for safe embedding in HTML attributes / text.
pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Strip characters that would break markdown image alt text / link syntax.
pub fn sanitize_alt(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '[' | ']' | '(' | ')' | '\n' | '\r'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Generate a random short rewrite span ID (8 hex chars).
fn generate_rewrite_id() -> String {
    // Take first 8 chars of a v4 UUID (32 bits of randomness, sufficient for
    // uniqueness within a single document).
    let id = uuid::Uuid::new_v4().to_string();
    id[..8].to_string()
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Create a new chapter file from raw markdown content.
pub fn create(content: &str) -> String {
    let fm = ChapterFrontmatter { next_note_id: 1, ..Default::default() };
    write_raw(&fm, content, &[])
}

/// Parse a chapter file and return its content and notes.
/// Footnote definitions are stripped from the returned content.
pub fn read(raw: &str) -> Result<EduPage, String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);
    let notes = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(EduPage { content, notes })
}

/// Append a note using an exact byte position.
///
/// `src_end` is the byte offset in the chapter content after which the
/// note anchor `[^<marker><id>]` is inserted.
pub fn add_note_at(
    raw: &str,
    note_type: NoteType,
    word: &str,
    src_end: usize,
    note_body: &str,
) -> Result<(String, NoteMeta), String> {
    let (mut fm, body) = parse_raw(raw)?;
    let (mut content, defs) = extract_footnote_defs(&body);

    let next_id = fm
        .next_note_id
        .max(fm.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1);
    fm.next_note_id = next_id + 1;
    let now = chrono::Utc::now().to_rfc3339();

    let anchor = anchor_text(&note_type, next_id);
    content = insert_at_byte(&content, src_end, &anchor)?;

    let new_note = NoteMeta {
        id: next_id,
        note_type: note_type.clone(),
        word: word.to_string(),
        ctime: now,
    };
    fm.notes.push(new_note.clone());

    let mut notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    // Set the body for the new note.
    if let Some(nwb) = notes_with_body.iter_mut().find(|n| n.id == next_id) {
        nwb.body = note_body.to_string();
    }

    Ok((write_raw(&fm, &content, &notes_with_body), new_note))
}

/// Remove a note: drops metadata, removes its anchor from content, and removes
/// its footnote definition.
pub fn delete_note(raw: &str, note_id: u32) -> Result<String, String> {
    let (mut fm, body) = parse_raw(raw)?;
    let (mut content, defs) = extract_footnote_defs(&body);

    let note_idx = fm
        .notes
        .iter()
        .position(|n| n.id == note_id)
        .ok_or_else(|| format!("note {} not found", note_id))?;
    let note_type = fm.notes[note_idx].note_type.clone();
    let anchor = anchor_text(&note_type, note_id);

    if !content.contains(&anchor) {
        return Err(format!("anchor for note {} not found in body", note_id));
    }
    content = content.replace(&anchor, "");
    fm.notes.remove(note_idx);

    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(write_raw(&fm, &content, &notes_with_body))
}

/// Rewrite the passage `src_start..src_end` (must be on a single line) to
/// `replacement`, wrapping it in a `<span data-rewrite-id="…">`.
/// Returns the new file content and the assigned rewrite ID.
pub fn rewrite_passage_at(
    raw: &str,
    src_start: usize,
    src_end: usize,
    replacement: &str,
) -> Result<(String, String), String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    if src_start > src_end || src_end > content.len() {
        return Err(format!(
            "invalid range {}..{} (source length {})",
            src_start,
            src_end,
            content.len()
        ));
    }

    let (start_line, line_start, line_end) = locate_byte(&content, src_start)?;
    let (end_line, _, _) = locate_byte(&content, src_end)?;

    if start_line != end_line {
        return Err("multi-line selections are not supported for rewrite".to_string());
    }

    let start_in_line = src_start - line_start;
    let end_in_line = src_end - line_start;
    let line = &content[line_start..line_end];

    if !line.is_char_boundary(start_in_line) || !line.is_char_boundary(end_in_line) {
        return Err("byte positions are not on UTF-8 character boundaries".to_string());
    }

    let rewrite_id = generate_rewrite_id();
    let span_text = format!(
        r#"<span data-rewrite-id="{}">{}</span>"#,
        rewrite_id,
        html_escape(replacement)
    );
    let new_line = format!(
        "{}{}{}",
        &line[..start_in_line],
        span_text,
        &line[end_in_line..]
    );
    let new_content = replace_line_content(&content, line_start, line_end, &new_line);

    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok((write_raw(&fm, &new_content, &notes_with_body), rewrite_id))
}

/// Replace an existing `<span data-rewrite-id="old_id">…</span>` with a new
/// span carrying a fresh ID and `new_replacement` as its content.
/// Returns `(new_raw, new_rewrite_id)`.
pub fn rewrite_existing_span(
    raw: &str,
    rewrite_id: &str,
    new_replacement: &str,
) -> Result<(String, String), String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    let open_marker = format!(r#"<span data-rewrite-id="{}">"#, rewrite_id);
    let close_marker = "</span>";
    let new_id = generate_rewrite_id();

    let mut new_content = content.clone();
    let mut found = false;

    let lines_owned: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    for (idx, line) in lines_owned.iter().enumerate() {
        if let Some(open_pos) = line.find(&open_marker) {
            let inner_start = open_pos + open_marker.len();
            if let Some(close_rel) = line[inner_start..].find(close_marker) {
                let span_end = inner_start + close_rel + close_marker.len();
                let new_span = format!(
                    r#"<span data-rewrite-id="{}">{}</span>"#,
                    new_id,
                    html_escape(new_replacement)
                );
                let new_line = format!("{}{}{}", &line[..open_pos], new_span, &line[span_end..]);
                let mut updated = lines_owned.clone();
                replace_line_by_idx(&mut updated, idx, &new_line);
                new_content = updated.join("\n");
                found = true;
                break;
            }
        }
    }

    if !found {
        return Err(format!("rewrite span {} not found", rewrite_id));
    }

    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok((write_raw(&fm, &new_content, &notes_with_body), new_id))
}

/// Insert an artifact reference `![alt](epar://sha1_id)` into the chapter.
///
/// `sha1_id` is the content-addressed artifact ID (e.g., `"abc123.svg"`).
/// Wide images (`aspect_ratio >= 1.0`) become block paragraphs after the
/// source line; tall images are inlined at `src_end`.
pub fn add_artifact_at(
    raw: &str,
    src_start: usize,
    src_end: usize,
    sha1_id: &str,
    caption: &str,
    aspect_ratio: f32,
    _semantic_type: &str,
) -> Result<String, String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    if src_start > src_end || src_end > content.len() {
        return Err(format!(
            "invalid range {}..{} (source length {})",
            src_start,
            src_end,
            content.len()
        ));
    }

    let alt = sanitize_alt(caption);
    let image_md = format!("![{}](epar://{})", alt, sha1_id);

    let new_content = if aspect_ratio >= 1.0 {
        // Wide: block paragraph after the source line.
        let (_, line_start, line_end) = locate_byte(&content, src_start)?;
        let line = &content[line_start..line_end];
        let new_block = format!("{}\n\n{}", line, image_md);
        replace_line_content(&content, line_start, line_end, &new_block)
    } else {
        // Tall: inline at src_end.
        insert_at_byte(&content, src_end, &format!(" {}", image_md))?
    };

    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(write_raw(&fm, &new_content, &notes_with_body))
}

/// Remove an artifact: strips the `![alt](epar://artifact_id)` from the body.
/// Block images (on their own paragraph) collapse back; inline images are
/// spliced out of their line.
pub fn delete_artifact(raw: &str, artifact_id: &str) -> Result<String, String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    let url = format!("(epar://{})", artifact_id);
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found = false;

    for idx in 0..lines.len() {
        let line = lines[idx].clone();
        if let Some(url_pos) = line.find(&url) {
            let bracket = line[..url_pos].rfind("![").ok_or("malformed image markdown")?;
            let img_end = url_pos + url.len();
            let mut img_start = bracket;
            if img_start > 0 && line.as_bytes()[img_start - 1] == b' ' {
                img_start -= 1;
            }
            let cleaned = format!("{}{}", &line[..img_start], &line[img_end..]);

            if cleaned.trim().is_empty() {
                // Block image: remove this line and the preceding blank line.
                if idx >= 1 && lines[idx - 1].trim().is_empty() {
                    lines.remove(idx); // remove image line first
                    lines.remove(idx - 1); // then blank line
                } else {
                    lines.remove(idx);
                }
            } else {
                lines[idx] = cleaned;
            }
            found = true;
            break;
        }
    }

    if !found {
        return Err(format!(
            "image marker for artifact {} not found in body",
            artifact_id
        ));
    }

    let new_content = lines.join("\n");
    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(write_raw(&fm, &new_content, &notes_with_body))
}

/// Update the artifact reference in the chapter when an artifact is regenerated.
///
/// Replaces `![alt](epar://old_id)` with `![new_alt](epar://new_id)` and
/// repositions between block and inline if the aspect ratio crosses the 1.0
/// boundary.
pub fn regenerate_artifact(
    raw: &str,
    old_id: &str,
    new_id: &str,
    new_caption: &str,
    new_aspect: f32,
) -> Result<String, String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    let old_url = format!("(epar://{})", old_id);
    let new_alt = sanitize_alt(new_caption);
    let new_image_md = format!("![{}](epar://{})", new_alt, new_id);

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found = false;

    for idx in 0..lines.len() {
        let line = lines[idx].clone();
        if let Some(url_pos) = line.find(&old_url) {
            let bracket = line[..url_pos].rfind("![").ok_or("malformed image markdown")?;
            let img_end = url_pos + old_url.len();
            let mut img_start = bracket;
            if img_start > 0 && line.as_bytes()[img_start - 1] == b' ' {
                img_start -= 1;
            }
            let cleaned = format!("{}{}", &line[..img_start], &line[img_end..]);
            let old_is_block = cleaned.trim().is_empty();
            let new_is_block = new_aspect >= 1.0;

            if old_is_block && new_is_block {
                // Block → Block: update the image reference.
                lines[idx] = new_image_md.clone();
            } else if old_is_block && !new_is_block {
                // Block → Inline: merge with preceding paragraph.
                if idx >= 1 && lines[idx - 1].trim().is_empty() && idx >= 2 {
                    // Pattern: [para, blank, image_line] → [para image]
                    let para = lines[idx - 2].clone();
                    lines[idx - 2] = format!("{} {}", para, new_image_md);
                    lines.remove(idx); // remove image line
                    lines.remove(idx - 1); // remove blank line
                } else {
                    lines[idx] = new_image_md.clone();
                }
            } else if !old_is_block && new_is_block {
                // Inline → Block: extract from line, add as new block paragraph.
                lines[idx] = cleaned.trim_end().to_string();
                lines.insert(idx + 1, String::new());
                lines.insert(idx + 2, new_image_md.clone());
            } else {
                // Inline → Inline: update the image reference within the line.
                lines[idx] = format!(
                    "{}{}{}",
                    &line[..img_start],
                    new_image_md,
                    &line[img_end..]
                );
            }
            found = true;
            break;
        }
    }

    if !found {
        return Err(format!(
            "image marker for artifact {} not found in body",
            old_id
        ));
    }

    let new_content = lines.join("\n");
    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(write_raw(&fm, &new_content, &notes_with_body))
}

/// Insert appendix cross-reference marker `[^A<seq>]` at `src_end`.
pub fn insert_appendix_ref_at(raw: &str, appendix_seq: u32, src_end: usize) -> Result<String, String> {
    let (fm, body) = parse_raw(raw)?;
    let (content, defs) = extract_footnote_defs(&body);

    let anchor = format!("[^A{}]", appendix_seq);
    let new_content = insert_at_byte(&content, src_end, &anchor)?;

    let notes_with_body = rebuild_notes_with_bodies(&fm.notes, &defs);
    Ok(write_raw(&fm, &new_content, &notes_with_body))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str =
        "# Chapter One\n\nHello world.\n\nSecond paragraph with blackhole here.";

    /// Find the byte position of the end of the nth occurrence of `target` in `s`.
    fn byte_end_of(s: &str, target: &str, occurrence: usize) -> usize {
        let mut count = 0;
        let mut start = 0;
        while let Some(pos) = s[start..].find(target) {
            count += 1;
            let end = start + pos + target.len();
            if count == occurrence {
                return end;
            }
            start += pos + 1;
        }
        panic!("target '{}' occurrence {} not found", target, occurrence);
    }

    // ── create / read ─────────────────────────────────────────────────────────

    #[test]
    fn create_produces_parseable_frontmatter() {
        let raw = create(SAMPLE);
        assert!(raw.starts_with("---\n"));
        let (fm, body) = parse_raw(&raw).unwrap();
        assert_eq!(fm.next_note_id, 1);
        assert!(fm.notes.is_empty());
        assert!(body.contains("# Chapter One"));
    }

    #[test]
    fn read_returns_content_and_empty_notes() {
        let raw = create(SAMPLE);
        let page = read(&raw).unwrap();
        assert_eq!(page.content, SAMPLE);
        assert!(page.notes.is_empty());
    }

    #[test]
    fn read_errors_on_missing_frontmatter() {
        assert!(read("just plain text without frontmatter").is_err());
    }

    // ── add_note_at ───────────────────────────────────────────────────────────

    #[test]
    fn add_note_at_inserts_anchor() {
        let content = "The blackhole is here.";
        let raw = create(content);
        let src_end = byte_end_of(content, "blackhole", 1);
        let (new_raw, note) =
            add_note_at(&raw, NoteType::Definition, "blackhole", src_end, "A spacetime region.").unwrap();
        let page = read(&new_raw).unwrap();
        assert_eq!(note.id, 1);
        assert_eq!(note.word, "blackhole");
        assert_eq!(page.content, "The blackhole[^*1] is here.");
        assert_eq!(page.notes[0].body, "A spacetime region.");
    }

    #[test]
    fn add_note_at_increments_id() {
        let content = "First blackhole. Second blackhole.";
        let raw = create(content);
        let e1 = byte_end_of(content, "blackhole", 1);
        let (raw, n1) = add_note_at(&raw, NoteType::Definition, "blackhole", e1, "A").unwrap();
        // After the first note the content changed; find the second occurrence in the new content.
        let new_content = read(&raw).unwrap().content;
        let e2 = byte_end_of(&new_content, "blackhole", 2);
        let (_, n2) = add_note_at(&raw, NoteType::Definition, "blackhole", e2, "B").unwrap();
        assert_eq!(n1.id, 1);
        assert_eq!(n2.id, 2);
    }

    #[test]
    fn add_note_at_footnote_uses_dagger() {
        let content = "The gravitational collapse is fast.";
        let raw = create(content);
        let e = byte_end_of(content, "gravitational collapse", 1);
        let (new_raw, _) =
            add_note_at(&raw, NoteType::Footnote, "gravitational collapse", e, "Body.").unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("[^†1]"));
    }

    #[test]
    fn add_note_does_not_reuse_id_after_delete() {
        let content = "A blackhole and another blackhole.";
        let raw = create(content);
        let e1 = byte_end_of(content, "blackhole", 1);
        let (raw, _) = add_note_at(&raw, NoteType::Definition, "blackhole", e1, "A").unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        let new_content = read(&raw).unwrap().content;
        let e2 = byte_end_of(&new_content, "blackhole", 1);
        let (_, note) = add_note_at(&raw, NoteType::Definition, "blackhole", e2, "B").unwrap();
        assert_eq!(note.id, 2, "id 1 must not be reused after deletion");
    }

    // ── delete_note ───────────────────────────────────────────────────────────

    #[test]
    fn delete_note_removes_anchor_and_body() {
        let content = "The blackhole is here.";
        let raw = create(content);
        let e = byte_end_of(content, "blackhole", 1);
        let (raw, _) = add_note_at(&raw, NoteType::Definition, "blackhole", e, "A region.").unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.content, content);
        assert!(page.notes.is_empty());
    }

    #[test]
    fn delete_note_preserves_other_notes() {
        let content = "First blackhole. Second blackhole.";
        let raw = create(content);
        let e1 = byte_end_of(content, "blackhole", 1);
        let (raw, _) = add_note_at(&raw, NoteType::Definition, "blackhole", e1, "First").unwrap();
        let c2 = read(&raw).unwrap().content;
        let e2 = byte_end_of(&c2, "blackhole", 2);
        let (raw, _) = add_note_at(&raw, NoteType::Definition, "blackhole", e2, "Second").unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].id, 2);
        assert_eq!(page.notes[0].body, "Second");
    }

    #[test]
    fn delete_note_errors_for_unknown_id() {
        let raw = create("Nothing here.");
        assert!(delete_note(&raw, 99).is_err());
    }

    // ── rewrite_passage_at ────────────────────────────────────────────────────

    #[test]
    fn rewrite_passage_wraps_in_span() {
        let content = "The collapse is dramatic.";
        let raw = create(content);
        let start = byte_end_of(content, "The ", 1);
        let end = byte_end_of(content, "collapse is dramatic", 1);
        let (new_raw, rid) = rewrite_passage_at(&raw, start, end, "fall is sudden").unwrap();
        assert_eq!(rid.len(), 8, "rewrite id should be 8 hex chars");
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains(&format!(r#"data-rewrite-id="{}""#, rid)));
        assert!(page.content.contains("fall is sudden"));
    }

    #[test]
    fn rewrite_passage_html_escapes_replacement() {
        let content = "See the demo here.";
        let raw = create(content);
        let start = byte_end_of(content, "See ", 1);
        let end = byte_end_of(content, "demo", 1);
        let (new_raw, _) = rewrite_passage_at(&raw, start, end, "X < Y & Z").unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("X &lt; Y &amp; Z"));
        assert!(!page.content.contains("X < Y"));
    }

    #[test]
    fn rewrite_passage_rejects_multi_line() {
        let content = "Line one.\nLine two.";
        let raw = create(content);
        let result = rewrite_passage_at(&raw, 0, content.len(), "X");
        assert!(result.is_err());
    }

    // ── rewrite_existing_span ─────────────────────────────────────────────────

    #[test]
    fn rewrite_existing_span_replaces_span() {
        let content = "The collapse is here.";
        let raw = create(content);
        let start = byte_end_of(content, "The ", 1);
        let end = byte_end_of(content, "collapse", 1);
        let (raw, old_id) = rewrite_passage_at(&raw, start, end, "first replacement").unwrap();
        let (new_raw, new_id) = rewrite_existing_span(&raw, &old_id, "second replacement").unwrap();
        assert_ne!(new_id, old_id);
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("second replacement"));
        assert!(!page.content.contains(&format!(r#"data-rewrite-id="{}""#, old_id)));
    }

    #[test]
    fn rewrite_existing_span_errors_for_unknown_id() {
        let raw = create("Just plain text.");
        assert!(rewrite_existing_span(&raw, "deadbeef", "X").is_err());
    }

    // ── add_artifact_at ───────────────────────────────────────────────────────

    #[test]
    fn add_artifact_wide_places_block_paragraph() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let new_raw = add_artifact_at(&raw, 0, src, "abc123.svg", "A star", 2.0, "image").unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("![A star](epar://abc123.svg)"));
        // Block image should be on its own paragraph.
        assert!(page.content.contains("\n\n![A star](epar://abc123.svg)"));
    }

    #[test]
    fn add_artifact_tall_places_inline() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let new_raw = add_artifact_at(&raw, 0, src, "abc123.svg", "Tall", 0.5, "image").unwrap();
        let page = read(&new_raw).unwrap();
        // Inline image inserted after "collapse".
        assert!(page.content.contains("collapse ![Tall](epar://abc123.svg)"));
    }

    #[test]
    fn add_artifact_sanitizes_alt_text() {
        let content = "The collapse here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let new_raw =
            add_artifact_at(&raw, 0, src, "abc.svg", "weird [brackets] (parens)", 2.0, "image")
                .unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("![weird brackets parens](epar://abc.svg)"));
    }

    // ── delete_artifact ───────────────────────────────────────────────────────

    #[test]
    fn delete_artifact_removes_block_image() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let new_raw =
            add_artifact_at(&raw, 0, src, "sha1a.svg", "cap", 2.0, "image").unwrap();
        let new_raw = delete_artifact(&new_raw, "sha1a.svg").unwrap();
        let page = read(&new_raw).unwrap();
        assert_eq!(page.content, content);
        assert!(!new_raw.contains("epar://sha1a.svg"));
    }

    #[test]
    fn delete_artifact_removes_inline_image() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let new_raw =
            add_artifact_at(&raw, 0, src, "sha1b.svg", "cap", 0.5, "image").unwrap();
        let new_raw = delete_artifact(&new_raw, "sha1b.svg").unwrap();
        let page = read(&new_raw).unwrap();
        assert_eq!(page.content, content);
    }

    #[test]
    fn delete_artifact_errors_for_unknown_id() {
        let raw = create("Nothing here.");
        assert!(delete_artifact(&raw, "nosuchid.svg").is_err());
    }

    // ── regenerate_artifact ───────────────────────────────────────────────────

    #[test]
    fn regenerate_artifact_updates_id_in_block() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let raw = add_artifact_at(&raw, 0, src, "old.svg", "cap", 2.0, "image").unwrap();
        let new_raw = regenerate_artifact(&raw, "old.svg", "new.svg", "new cap", 2.5).unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.content.contains("epar://new.svg"));
        assert!(!page.content.contains("epar://old.svg"));
    }

    #[test]
    fn regenerate_artifact_block_to_inline() {
        let content = "The collapse is shown here.";
        let raw = create(content);
        let src = byte_end_of(content, "collapse", 1);
        let raw = add_artifact_at(&raw, 0, src, "old.svg", "cap", 2.0, "image").unwrap();
        let new_raw = regenerate_artifact(&raw, "old.svg", "new.svg", "tall", 0.5).unwrap();
        let page = read(&new_raw).unwrap();
        // Should now be inline.
        assert!(page.content.contains("epar://new.svg"));
        assert!(!page.content.contains("\n\n![tall](epar://new.svg)"));
    }

    #[test]
    fn regenerate_artifact_errors_for_unknown_id() {
        let raw = create("Nothing here.");
        assert!(regenerate_artifact(&raw, "nosuch.svg", "new.svg", "cap", 1.0).is_err());
    }

    // ── insert_appendix_ref_at ────────────────────────────────────────────────

    #[test]
    fn insert_appendix_ref_adds_marker() {
        let content = "See the gravitational collapse here.";
        let raw = create(content);
        let e = byte_end_of(content, "gravitational collapse", 1);
        let new_raw = insert_appendix_ref_at(&raw, 1, e).unwrap();
        let page = read(&new_raw).unwrap();
        assert_eq!(
            page.content,
            "See the gravitational collapse[^A1] here."
        );
    }

    // ── footnote def parsing ──────────────────────────────────────────────────

    #[test]
    fn extract_footnote_defs_handles_definition_and_footnote() {
        let body = "Some text.[^*1]\n\n[^*1]: Definition body.\n[^†2]: Footnote body.";
        let (content, defs) = extract_footnote_defs(body);
        assert_eq!(content, "Some text.[^*1]");
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0], ('*', 1, "Definition body.".to_string()));
        assert_eq!(defs[1], ('†', 2, "Footnote body.".to_string()));
    }

    #[test]
    fn extract_footnote_defs_handles_multiline_body() {
        let body = "Content.\n\n[^*1]: First line.\n    Continuation.";
        let (content, defs) = extract_footnote_defs(body);
        assert_eq!(content, "Content.");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].2, "First line.\nContinuation.");
    }

    #[test]
    fn round_trip_note_body_preserved() {
        let content = "The blackhole is here.";
        let raw = create(content);
        let e = byte_end_of(content, "blackhole", 1);
        let (new_raw, _) =
            add_note_at(&raw, NoteType::Definition, "blackhole", e, "A region of spacetime.").unwrap();
        let page = read(&new_raw).unwrap();
        assert_eq!(page.notes[0].body, "A region of spacetime.");
    }

    // ── parse_footnote_def_header ─────────────────────────────────────────────

    #[test]
    fn parse_def_header_asterisk() {
        let (m, id, body) = parse_footnote_def_header("[^*3]: Some text.").unwrap();
        assert_eq!(m, '*');
        assert_eq!(id, 3);
        assert_eq!(body, "Some text.");
    }

    #[test]
    fn parse_def_header_dagger() {
        let (m, id, _) = parse_footnote_def_header("[^†10]: Body.").unwrap();
        assert_eq!(m, '†');
        assert_eq!(id, 10);
    }

    #[test]
    fn parse_def_header_rejects_regular_text() {
        assert!(parse_footnote_def_header("[^A1]: Appendix.").is_none());
        assert!(parse_footnote_def_header("Just a line.").is_none());
    }
}
