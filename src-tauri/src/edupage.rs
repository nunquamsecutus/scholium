use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdupageHeader {
    pub version: u32,
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub revisions: Vec<RevisionMeta>,
    pub assets: Vec<AssetMeta>,
    #[serde(default)]
    pub notes: Vec<NoteMeta>,
    /// Monotonically increasing id allocator for notes; never decreases, so
    /// deleted ids are never re-issued.
    #[serde(default = "default_next_note_id")]
    pub next_note_id: u32,
    /// Monotonically increasing id allocator for rewrite spans. Same rule
    /// as next_note_id — ids are never reused.
    #[serde(default = "default_next_rewrite_id")]
    pub next_rewrite_id: u32,
    #[serde(default)]
    pub artifacts: Vec<ArtifactMeta>,
    /// Monotonically increasing id allocator for artifacts. Never reused.
    #[serde(default = "default_next_artifact_id")]
    pub next_artifact_id: u32,
}

fn default_next_note_id() -> u32 {
    1
}

fn default_next_rewrite_id() -> u32 {
    1
}

fn default_next_artifact_id() -> u32 {
    1
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionMeta {
    pub id: String,
    pub ctime: String,
    pub action_id: String,
    #[serde(rename = "type")]
    pub revision_type: RevisionType,
    pub line_start: usize,
    pub line_end: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RevisionType {
    #[serde(rename = "ADD")]
    Add,
    #[serde(rename = "EDIT")]
    Edit,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMeta {
    pub id: String,
    pub ctime: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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

/// The marker character that follows `[^` in the source-level anchor for a
/// note of the given type. Asterisk for definitions, dagger for footnotes,
/// double-dagger for endnotes.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactMeta {
    pub id: u32,
    pub mime_type: String,
    /// "image" for now; later "diagram", "chart", etc.
    pub semantic_type: String,
    pub ctime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// width / height from the SVG viewBox, precomputed so the renderer
    /// doesn't have to re-parse on every load. Drives block vs float layout.
    pub aspect_ratio: f32,
    /// The text the artifact was generated from (for re-generation later).
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactWithBody {
    pub id: u32,
    pub mime_type: String,
    pub semantic_type: String,
    pub ctime: String,
    pub caption: Option<String>,
    pub aspect_ratio: f32,
    pub source: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct EduPage {
    pub content: String,
    pub notes: Vec<NoteWithBody>,
    pub artifacts: Vec<ArtifactWithBody>,
}

fn sha1_hex(content: &str) -> String {
    let mut h = Sha1::new();
    h.update(content.as_bytes());
    format!("{:x}", h.finalize())
}

fn delimiter(file_id: &str, sha1: &str) -> String {
    format!("======! {}|{} !======", file_id, sha1)
}

fn note_delimiter(file_id: &str, note_id: u32) -> String {
    format!("======! {}|NOTE:{} !======", file_id, note_id)
}

fn artifact_delimiter(file_id: &str, artifact_id: u32) -> String {
    format!("======! {}|ARTIFACT:{} !======", file_id, artifact_id)
}

fn parse_delimiter(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix("======!")?.strip_suffix("!======")?;
    let inner = inner.trim();
    inner
        .split_once('|')
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '-'
}

/// Remove the inline markdown emphasis markers (`*`, `_`, `` ` ``, `~`) from a
/// source line so that text matching can be done against text-as-rendered.
/// Returns the stripped string and a byte map where `byte_map[i]` is the
/// source byte position of stripped byte `i`, plus a sentinel at the end so
/// `byte_map[stripped.len()] == line.len()`.
fn strip_inline_markdown(line: &str) -> (String, Vec<usize>) {
    let mut stripped = String::new();
    let mut byte_map: Vec<usize> = Vec::with_capacity(line.len() + 1);
    let mut src_byte = 0usize;
    for ch in line.chars() {
        let ch_len = ch.len_utf8();
        if matches!(ch, '*' | '_' | '`' | '~') {
            src_byte += ch_len;
            continue;
        }
        for k in 0..ch_len {
            byte_map.push(src_byte + k);
        }
        stripped.push(ch);
        src_byte += ch_len;
    }
    byte_map.push(src_byte);
    (stripped, byte_map)
}

pub fn create(file_id: &str, title: &str, description: Option<&str>, content: &str) -> String {
    let sha1 = sha1_hex(content);
    let ctime = chrono::Utc::now().to_rfc3339();
    let action_id = uuid::Uuid::new_v4().to_string();
    let line_end = content.lines().count();

    let header = EdupageHeader {
        version: 1,
        id: file_id.to_string(),
        title: title.to_string(),
        description: description.map(str::to_string),
        revisions: vec![RevisionMeta {
            id: sha1.clone(),
            ctime,
            action_id,
            revision_type: RevisionType::Add,
            line_start: 1,
            line_end: Some(line_end),
        }],
        assets: vec![],
        notes: vec![],
        next_note_id: 1,
        next_rewrite_id: 1,
        artifacts: vec![],
        next_artifact_id: 1,
    };

    let header_json = serde_json::to_string_pretty(&header).expect("edupage header serialization");
    format!("{}\n{}\n{}", header_json, delimiter(file_id, &sha1), content)
}

/// Parse the header and all delimited blocks. Block keys are either revision
/// sha1s or `NOTE:<id>` markers.
fn parse_blocks(raw: &str) -> Result<(EdupageHeader, HashMap<String, String>), String> {
    let lines: Vec<&str> = raw.lines().collect();
    let delimiters: Vec<(usize, String, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_delimiter(line).map(|(uuid, key)| (i, uuid, key)))
        .collect();

    if delimiters.is_empty() {
        return Err("no revision delimiters found in edupage".to_string());
    }

    let header_end = delimiters[0].0;
    let header: EdupageHeader = serde_json::from_str(&lines[..header_end].join("\n"))
        .map_err(|e| format!("invalid edupage header: {e}"))?;

    let mut blocks: HashMap<String, String> = HashMap::new();
    for (idx, (line_idx, _, key)) in delimiters.iter().enumerate() {
        let start = line_idx + 1;
        let end = if idx + 1 < delimiters.len() {
            delimiters[idx + 1].0
        } else {
            lines.len()
        };
        blocks.insert(key.clone(), lines[start..end].join("\n"));
    }

    Ok((header, blocks))
}

pub fn reconstruct(raw: &str) -> Result<String, String> {
    let (header, blocks) = parse_blocks(raw)?;

    let mut doc: Vec<String> = Vec::new();
    for rev in &header.revisions {
        let content = blocks
            .get(&rev.id)
            .ok_or_else(|| format!("missing content block for revision {}", rev.id))?;
        let new_lines: Vec<String> = content.lines().map(str::to_string).collect();

        match rev.revision_type {
            RevisionType::Add => {
                let at = (rev.line_start - 1).min(doc.len());
                for (i, line) in new_lines.into_iter().enumerate() {
                    doc.insert(at + i, line);
                }
            }
            RevisionType::Edit => {
                let start = (rev.line_start - 1).min(doc.len());
                let end = rev.line_end.map(|e| e.min(doc.len())).unwrap_or(start);
                doc.splice(start..end, new_lines);
            }
        }
    }

    Ok(doc.join("\n"))
}

/// Read the full edupage: reconstructed markdown content plus notes with
/// their bodies, joined by id.
pub fn read(raw: &str) -> Result<EduPage, String> {
    let (header, blocks) = parse_blocks(raw)?;
    let content = reconstruct(raw)?;

    let notes: Vec<NoteWithBody> = header
        .notes
        .iter()
        .map(|meta| {
            let key = format!("NOTE:{}", meta.id);
            let body = blocks.get(&key).cloned().unwrap_or_default();
            NoteWithBody {
                id: meta.id,
                note_type: meta.note_type.clone(),
                word: meta.word.clone(),
                ctime: meta.ctime.clone(),
                body,
            }
        })
        .collect();

    let artifacts: Vec<ArtifactWithBody> = header
        .artifacts
        .iter()
        .map(|meta| {
            let key = format!("ARTIFACT:{}", meta.id);
            let body = blocks.get(&key).cloned().unwrap_or_default();
            ArtifactWithBody {
                id: meta.id,
                mime_type: meta.mime_type.clone(),
                semantic_type: meta.semantic_type.clone(),
                ctime: meta.ctime.clone(),
                caption: meta.caption.clone(),
                aspect_ratio: meta.aspect_ratio,
                source: meta.source.clone(),
                body,
            }
        })
        .collect();

    Ok(EduPage {
        content,
        notes,
        artifacts,
    })
}

/// Insert `anchor` after the `target_n`-th word-bounded occurrence of
/// `query` in `content`. `query` may be a single word or a multi-word phrase
/// — word-boundary checks apply to the character immediately preceding and
/// following the match. Returns the 0-indexed line number that changed and
/// the new line text.
fn insert_anchor(
    content: &str,
    query: &str,
    target_n: u32,
    anchor: &str,
) -> Result<(usize, String), String> {
    if target_n == 0 {
        return Err("occurrence index must be 1 or greater".to_string());
    }
    let mut count: u32 = 0;
    for (line_idx, line) in content.lines().enumerate() {
        // Search against the line with inline markdown markers stripped so
        // the rendered-text selection matches even when the source has
        // `**bold**`, `_italic_`, etc. byte_map[i] gives the source position
        // of stripped byte i (with a sentinel at the end).
        let (stripped, byte_map) = strip_inline_markdown(line);
        let mut search_start = 0;
        while let Some(rel_pos) = stripped[search_start..].find(query) {
            let pos = search_start + rel_pos;
            let end = pos + query.len();

            let before_is_word = pos > 0
                && stripped[..pos]
                    .chars()
                    .last()
                    .map(is_word_char)
                    .unwrap_or(false);
            let after_is_word = end < stripped.len()
                && stripped[end..]
                    .chars()
                    .next()
                    .map(is_word_char)
                    .unwrap_or(false);

            if !before_is_word && !after_is_word {
                count += 1;
                if count == target_n {
                    let src_end = byte_map[end];
                    let new_line =
                        format!("{}{}{}", &line[..src_end], anchor, &line[src_end..]);
                    return Ok((line_idx, new_line));
                }
            }

            search_start = pos + 1;
        }
    }
    Err(format!(
        "could not find occurrence {} of '{}'",
        target_n, query
    ))
}

/// Append a note to the edupage: assigns the next id, inserts the
/// `[^*<id>]` anchor at the requested occurrence as an EDIT revision, and
/// stores the body content in a new NOTE block.
pub fn add_note(
    raw: &str,
    note_type: NoteType,
    word: &str,
    occurrence_index: u32,
    note_body: &str,
) -> Result<(String, NoteMeta), String> {
    let (mut header, _blocks) = parse_blocks(raw)?;

    // Belt and braces: respect both the counter and any existing ids in case
    // a file was hand-edited or comes from an older format without the counter.
    let next_id = header
        .next_note_id
        .max(header.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1);
    header.next_note_id = next_id + 1;
    let now = chrono::Utc::now().to_rfc3339();

    // Reconstruct current markdown content so we can locate the target.
    let content = reconstruct(raw)?;
    let anchor = anchor_text(&note_type, next_id);
    let (line_idx, new_line) = insert_anchor(&content, word, occurrence_index, &anchor)?;
    let line_number = line_idx + 1; // 1-indexed for RevisionMeta

    let new_sha1 = sha1_hex(&new_line);
    let file_id = header.id.clone();

    let new_revision = RevisionMeta {
        id: new_sha1.clone(),
        ctime: now.clone(),
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    };

    let new_note = NoteMeta {
        id: next_id,
        note_type,
        word: word.to_string(),
        ctime: now,
    };

    header.revisions.push(new_revision);
    header.notes.push(new_note.clone());

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");

    // Body of the file = everything from the first delimiter onward, untouched.
    let lines: Vec<&str> = raw.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| parse_delimiter(l).is_some())
        .ok_or("no delimiter in edupage")?;
    let existing_body = lines[body_start..].join("\n");

    let mut new_file = header_json;
    new_file.push('\n');
    new_file.push_str(&existing_body);
    new_file.push('\n');
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_line);
    new_file.push('\n');
    new_file.push_str(&note_delimiter(&file_id, next_id));
    new_file.push('\n');
    new_file.push_str(note_body);

    Ok((new_file, new_note))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Replace the `target_n`-th word-bounded occurrence of `selection` on a
/// single source line with `replacement`. Errors if the selection contains
/// a newline (multi-line rewrites aren't supported yet). Returns the
/// 0-indexed line that changed and its new text.
fn replace_in_line(
    content: &str,
    selection: &str,
    target_n: u32,
    replacement: &str,
) -> Result<(usize, String), String> {
    if selection.contains('\n') {
        return Err("multi-line selections aren't supported for rewrite yet".to_string());
    }
    if target_n == 0 {
        return Err("occurrence index must be 1 or greater".to_string());
    }
    let mut count: u32 = 0;
    for (line_idx, line) in content.lines().enumerate() {
        let (stripped, byte_map) = strip_inline_markdown(line);
        let mut search_start = 0;
        while let Some(rel_pos) = stripped[search_start..].find(selection) {
            let pos = search_start + rel_pos;
            let end = pos + selection.len();

            let before_is_word = pos > 0
                && stripped[..pos]
                    .chars()
                    .last()
                    .map(is_word_char)
                    .unwrap_or(false);
            let after_is_word = end < stripped.len()
                && stripped[end..]
                    .chars()
                    .next()
                    .map(is_word_char)
                    .unwrap_or(false);

            if !before_is_word && !after_is_word {
                count += 1;
                if count == target_n {
                    let src_pos = byte_map[pos];
                    let src_end = byte_map[end];
                    let new_line = format!(
                        "{}{}{}",
                        &line[..src_pos],
                        replacement,
                        &line[src_end..]
                    );
                    return Ok((line_idx, new_line));
                }
            }

            search_start = pos + 1;
        }
    }
    Err(format!(
        "could not find occurrence {} of '{}'",
        target_n, selection
    ))
}

/// Rewrite the Nth occurrence of `selection` to `replacement`, wrapping
/// the replacement in `<span data-rewrite-id="N">…</span>` so the renderer
/// (and selection-touches-rewrite detection) can recognize it later.
/// Records the change as a single-line EDIT revision and increments the
/// next_rewrite_id counter. Returns the new file content and the assigned
/// rewrite id.
pub fn rewrite_passage(
    raw: &str,
    selection: &str,
    occurrence_index: u32,
    replacement: &str,
) -> Result<(String, u32), String> {
    let (mut header, _) = parse_blocks(raw)?;
    let rewrite_id = header.next_rewrite_id;
    header.next_rewrite_id = rewrite_id + 1;
    let now = chrono::Utc::now().to_rfc3339();

    let content = reconstruct(raw)?;
    let span_text = format!(
        r#"<span data-rewrite-id="{}">{}</span>"#,
        rewrite_id,
        html_escape(replacement),
    );
    let (line_idx, new_line) =
        replace_in_line(&content, selection, occurrence_index, &span_text)?;
    let line_number = line_idx + 1;

    let new_sha1 = sha1_hex(&new_line);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now,
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    });

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let lines: Vec<&str> = raw.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| parse_delimiter(l).is_some())
        .ok_or("no delimiter in edupage")?;
    let existing_body = lines[body_start..].join("\n");

    let mut new_file = header_json;
    new_file.push('\n');
    new_file.push_str(&existing_body);
    new_file.push('\n');
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_line);

    Ok((new_file, rewrite_id))
}

/// Replace the entire `<span data-rewrite-id="<rewrite_id>">…</span>` with a
/// new span carrying the next monotonic id and `new_replacement` as its
/// content. Recorded as a single-line EDIT revision. Assumes the rewrite
/// span sits on a single line (true for content produced by
/// `rewrite_passage`).
pub fn rewrite_existing_span(
    raw: &str,
    rewrite_id: u32,
    new_replacement: &str,
) -> Result<(String, u32), String> {
    let (mut header, _) = parse_blocks(raw)?;
    let new_id = header.next_rewrite_id;
    header.next_rewrite_id = new_id + 1;
    let now = chrono::Utc::now().to_rfc3339();

    let content = reconstruct(raw)?;
    let open_marker = format!(r#"<span data-rewrite-id="{}">"#, rewrite_id);
    let close_marker = "</span>";

    let mut found: Option<(usize, String)> = None;
    for (idx, line) in content.lines().enumerate() {
        if let Some(open_pos) = line.find(&open_marker) {
            let inner_start = open_pos + open_marker.len();
            if let Some(close_rel) = line[inner_start..].find(close_marker) {
                let span_end = inner_start + close_rel + close_marker.len();
                let new_span = format!(
                    r#"<span data-rewrite-id="{}">{}</span>"#,
                    new_id,
                    html_escape(new_replacement),
                );
                let new_line =
                    format!("{}{}{}", &line[..open_pos], new_span, &line[span_end..]);
                found = Some((idx, new_line));
                break;
            }
        }
    }
    let (line_idx, new_line) =
        found.ok_or_else(|| format!("rewrite span {} not found", rewrite_id))?;

    let line_number = line_idx + 1;
    let new_sha1 = sha1_hex(&new_line);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now,
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    });

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let lines: Vec<&str> = raw.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| parse_delimiter(l).is_some())
        .ok_or("no delimiter in edupage")?;
    let existing_body = lines[body_start..].join("\n");

    let mut new_file = header_json;
    new_file.push('\n');
    new_file.push_str(&existing_body);
    new_file.push('\n');
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_line);

    Ok((new_file, new_id))
}

/// Remove an artifact: drop its metadata, strip the ARTIFACT block, and
/// remove the `![alt](epar://<id>)` image from the body via an EDIT revision.
/// Block images (on their own paragraph) collapse back to the preceding
/// paragraph; inline images are spliced out of their line (consuming the
/// leading space they were inserted with).
pub fn delete_artifact(raw: &str, artifact_id: u32) -> Result<String, String> {
    let (mut header, _) = parse_blocks(raw)?;
    let art_idx = header
        .artifacts
        .iter()
        .position(|a| a.id == artifact_id)
        .ok_or_else(|| format!("artifact {} not found", artifact_id))?;
    let now = chrono::Utc::now().to_rfc3339();
    let content = reconstruct(raw)?;
    let lines: Vec<&str> = content.lines().collect();
    let url = format!("(epar://{})", artifact_id);

    // (start_0, end_0_exclusive, new_lines) for the EDIT revision.
    let mut edit: Option<(usize, usize, Vec<String>)> = None;
    for (i, line) in lines.iter().enumerate() {
        let Some(url_pos) = line.find(&url) else { continue };
        let bracket = line[..url_pos]
            .rfind("![")
            .ok_or("malformed image markdown")?;
        let img_end = url_pos + url.len();
        let mut img_start = bracket;
        if img_start > 0 && line.as_bytes()[img_start - 1] == b' ' {
            img_start -= 1;
        }
        let cleaned = format!("{}{}", &line[..img_start], &line[img_end..]);
        if cleaned.trim().is_empty() {
            // Block image: collapse [paragraph, blank, image] back to [paragraph].
            if i >= 2 && lines[i - 1].trim().is_empty() {
                edit = Some((i - 2, i + 1, vec![lines[i - 2].to_string()]));
            } else {
                edit = Some((i, i + 1, vec![]));
            }
        } else {
            edit = Some((i, i + 1, vec![cleaned]));
        }
        break;
    }
    let (start_0, end_0, new_lines) = edit
        .ok_or_else(|| format!("image marker for artifact {} not found in body", artifact_id))?;

    let new_block = new_lines.join("\n");
    let new_sha1 = sha1_hex(&new_block);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now,
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: start_0 + 1,
        line_end: Some(end_0),
    });
    header.artifacts.remove(art_idx);

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let deleted_key = format!("ARTIFACT:{}", artifact_id);
    let raw_lines: Vec<&str> = raw.lines().collect();
    let delimiters: Vec<(usize, String, String)> = raw_lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_delimiter(line).map(|(uuid, key)| (i, uuid, key)))
        .collect();

    let mut new_file = header_json;
    new_file.push('\n');
    for (idx, (line_idx, _, key)) in delimiters.iter().enumerate() {
        if key == &deleted_key {
            continue;
        }
        let start = *line_idx;
        let end = if idx + 1 < delimiters.len() {
            delimiters[idx + 1].0
        } else {
            raw_lines.len()
        };
        new_file.push_str(&raw_lines[start..end].join("\n"));
        new_file.push('\n');
    }
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_block);

    Ok(new_file)
}

/// Replace an artifact's SVG and caption, updating its aspect ratio. If the
/// aspect ratio crosses the block/float boundary (>= 1 vs < 1) the body
/// marker is repositioned with an EDIT revision: block → inline appends the
/// image to its paragraph; inline → block lifts it onto its own paragraph.
/// Otherwise only the ARTIFACT block and metadata change.
pub fn regenerate_artifact(
    raw: &str,
    artifact_id: u32,
    new_svg: &str,
    new_caption: &str,
    new_aspect: f32,
) -> Result<(String, ArtifactMeta), String> {
    let (mut header, _) = parse_blocks(raw)?;
    let idx = header
        .artifacts
        .iter()
        .position(|a| a.id == artifact_id)
        .ok_or_else(|| format!("artifact {} not found", artifact_id))?;
    let old_wide = header.artifacts[idx].aspect_ratio >= 1.0;
    let new_wide = new_aspect >= 1.0;
    let now = chrono::Utc::now().to_rfc3339();

    header.artifacts[idx].aspect_ratio = new_aspect;
    header.artifacts[idx].caption = if new_caption.trim().is_empty() {
        None
    } else {
        Some(new_caption.trim().to_string())
    };
    let updated = header.artifacts[idx].clone();
    let file_id = header.id.clone();

    // Reposition the body marker only when the placement class flips.
    let mut extra_edit: Option<(String, String)> = None;
    if old_wide != new_wide {
        let content = reconstruct(raw)?;
        let lines: Vec<&str> = content.lines().collect();
        let url = format!("(epar://{})", artifact_id);
        let alt = sanitize_alt(new_caption);
        let image_md = format!("![{}](epar://{})", alt, artifact_id);

        let mut found: Option<(usize, usize, Vec<String>)> = None;
        for (i, line) in lines.iter().enumerate() {
            let Some(url_pos) = line.find(&url) else { continue };
            let bracket = line[..url_pos]
                .rfind("![")
                .ok_or("malformed image markdown")?;
            let img_end = url_pos + url.len();
            let mut img_start = bracket;
            if img_start > 0 && line.as_bytes()[img_start - 1] == b' ' {
                img_start -= 1;
            }
            let cleaned = format!("{}{}", &line[..img_start], &line[img_end..]);
            if new_wide {
                // inline → block: drop the inline image, lift onto its own paragraph.
                let para = cleaned.trim_end().to_string();
                found = Some((i, i + 1, vec![para, String::new(), image_md.clone()]));
            } else if i >= 2 && lines[i - 1].trim().is_empty() {
                // block → inline: append to the preceding paragraph.
                found = Some((i - 2, i + 1, vec![format!("{} {}", lines[i - 2], image_md)]));
            } else {
                found = Some((i, i + 1, vec![image_md.clone()]));
            }
            break;
        }
        let (start_0, end_0, new_lines) = found
            .ok_or_else(|| format!("image marker for artifact {} not found", artifact_id))?;
        let block = new_lines.join("\n");
        let sha1 = sha1_hex(&block);
        header.revisions.push(RevisionMeta {
            id: sha1.clone(),
            ctime: now,
            action_id: uuid::Uuid::new_v4().to_string(),
            revision_type: RevisionType::Edit,
            line_start: start_0 + 1,
            line_end: Some(end_0),
        });
        extra_edit = Some((sha1, block));
    }

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let artifact_key = format!("ARTIFACT:{}", artifact_id);
    let raw_lines: Vec<&str> = raw.lines().collect();
    let delimiters: Vec<(usize, String, String)> = raw_lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_delimiter(line).map(|(uuid, key)| (i, uuid, key)))
        .collect();

    let mut new_file = header_json;
    new_file.push('\n');
    for (di, (line_idx, _, key)) in delimiters.iter().enumerate() {
        let start = *line_idx;
        let end = if di + 1 < delimiters.len() {
            delimiters[di + 1].0
        } else {
            raw_lines.len()
        };
        if key == &artifact_key {
            // Keep the delimiter line, swap the block body for the new SVG.
            new_file.push_str(raw_lines[start]);
            new_file.push('\n');
            new_file.push_str(new_svg);
            new_file.push('\n');
        } else {
            new_file.push_str(&raw_lines[start..end].join("\n"));
            new_file.push('\n');
        }
    }
    if let Some((sha1, block)) = extra_edit {
        new_file.push_str(&delimiter(&file_id, &sha1));
        new_file.push('\n');
        new_file.push_str(&block);
    }

    Ok((new_file, updated))
}

/// Return the 0-indexed line containing the `target_n`-th word-bounded
/// occurrence of `query`. Matches against inline-markdown-stripped lines so
/// selections of formatted text find their source line.
fn line_of_occurrence(content: &str, query: &str, target_n: u32) -> Result<usize, String> {
    let mut count: u32 = 0;
    for (line_idx, line) in content.lines().enumerate() {
        let (stripped, _byte_map) = strip_inline_markdown(line);
        let mut search_start = 0;
        while let Some(rel_pos) = stripped[search_start..].find(query) {
            let pos = search_start + rel_pos;
            let end = pos + query.len();
            let before_is_word = pos > 0
                && stripped[..pos].chars().last().map(is_word_char).unwrap_or(false);
            let after_is_word = end < stripped.len()
                && stripped[end..].chars().next().map(is_word_char).unwrap_or(false);
            if !before_is_word && !after_is_word {
                count += 1;
                if count == target_n {
                    return Ok(line_idx);
                }
            }
            search_start = pos + 1;
        }
    }
    Err(format!(
        "could not find occurrence {} of '{}'",
        target_n, query
    ))
}

/// Strip characters that would break markdown image alt text / link syntax.
fn sanitize_alt(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '[' | ']' | '(' | ')' | '\n' | '\r'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Add an SVG artifact: store the SVG in an ARTIFACT block, record metadata
/// in the header, and place a markdown image (`![alt](epar://<id>)`) near the
/// Nth occurrence of `selection`. Wide images (aspect_ratio >= 1) go on their
/// own paragraph after the source line; tall images go inline right after the
/// selection so the renderer can float them beside the text. Recorded as a
/// single EDIT revision (which may expand one source line into several).
pub fn add_artifact(
    raw: &str,
    selection: &str,
    occurrence_index: u32,
    svg: &str,
    caption: &str,
    aspect_ratio: f32,
    semantic_type: &str,
) -> Result<(String, ArtifactMeta), String> {
    let (mut header, _) = parse_blocks(raw)?;
    let artifact_id = header.next_artifact_id;
    header.next_artifact_id = artifact_id + 1;
    let now = chrono::Utc::now().to_rfc3339();

    let content = reconstruct(raw)?;
    let alt = sanitize_alt(caption);
    let image_md = format!("![{}](epar://{})", alt, artifact_id);

    let (line_idx, new_block) = if aspect_ratio >= 1.0 {
        // Wide: a block image on its own paragraph after the source line.
        let idx = line_of_occurrence(&content, selection, occurrence_index)?;
        let line = content.lines().nth(idx).unwrap_or("");
        (idx, format!("{}\n\n{}", line, image_md))
    } else {
        // Tall: inline right after the selection, so it can float beside text.
        insert_anchor(&content, selection, occurrence_index, &format!(" {}", image_md))?
    };
    let line_number = line_idx + 1;

    let new_sha1 = sha1_hex(&new_block);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now.clone(),
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    });

    let artifact = ArtifactMeta {
        id: artifact_id,
        mime_type: "image/svg+xml".to_string(),
        semantic_type: semantic_type.to_string(),
        ctime: now,
        caption: if caption.trim().is_empty() {
            None
        } else {
            Some(caption.trim().to_string())
        },
        aspect_ratio,
        source: selection.to_string(),
    };
    header.artifacts.push(artifact.clone());

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let lines: Vec<&str> = raw.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| parse_delimiter(l).is_some())
        .ok_or("no delimiter in edupage")?;
    let existing_body = lines[body_start..].join("\n");

    let mut new_file = header_json;
    new_file.push('\n');
    new_file.push_str(&existing_body);
    new_file.push('\n');
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_block);
    new_file.push('\n');
    new_file.push_str(&artifact_delimiter(&file_id, artifact_id));
    new_file.push('\n');
    new_file.push_str(svg);

    Ok((new_file, artifact))
}

/// Insert a `[^A<appendix_seq>]` cross-reference marker after the Nth
/// occurrence of `selection` in the body, recorded as an EDIT revision.
/// Unlike notes, the appendix link points to another chapter and has no
/// metadata stored in this file's header — the link target is the manifest
/// entry whose id is `ap-<seq>`.
pub fn insert_appendix_ref(
    raw: &str,
    appendix_seq: u32,
    selection: &str,
    occurrence_index: u32,
) -> Result<String, String> {
    let (mut header, _) = parse_blocks(raw)?;
    let now = chrono::Utc::now().to_rfc3339();
    let content = reconstruct(raw)?;
    let anchor = format!("[^A{}]", appendix_seq);
    let (line_idx, new_line) = insert_anchor(&content, selection, occurrence_index, &anchor)?;
    let line_number = line_idx + 1;
    let new_sha1 = sha1_hex(&new_line);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now,
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    });

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let lines: Vec<&str> = raw.lines().collect();
    let body_start = lines
        .iter()
        .position(|l| parse_delimiter(l).is_some())
        .ok_or("no delimiter in edupage")?;
    let existing_body = lines[body_start..].join("\n");

    let mut new_file = header_json;
    new_file.push('\n');
    new_file.push_str(&existing_body);
    new_file.push('\n');
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_line);

    Ok(new_file)
}

/// Remove a note: drops the metadata, strips the NOTE block, and appends a
/// new EDIT revision that removes the `[^*<id>]` anchor from the body.
pub fn delete_note(raw: &str, note_id: u32) -> Result<String, String> {
    let (mut header, _blocks) = parse_blocks(raw)?;

    let note_idx = header
        .notes
        .iter()
        .position(|n| n.id == note_id)
        .ok_or_else(|| format!("note {} not found", note_id))?;
    let note_type = header.notes[note_idx].note_type.clone();

    let now = chrono::Utc::now().to_rfc3339();
    let content = reconstruct(raw)?;
    let anchor = anchor_text(&note_type, note_id);

    let (line_idx, new_line) = content
        .lines()
        .enumerate()
        .find_map(|(i, line)| {
            if line.contains(&anchor) {
                Some((i, line.replace(&anchor, "")))
            } else {
                None
            }
        })
        .ok_or_else(|| format!("anchor for note {} not found in body", note_id))?;
    let line_number = line_idx + 1;

    let new_sha1 = sha1_hex(&new_line);
    let file_id = header.id.clone();

    header.revisions.push(RevisionMeta {
        id: new_sha1.clone(),
        ctime: now,
        action_id: uuid::Uuid::new_v4().to_string(),
        revision_type: RevisionType::Edit,
        line_start: line_number,
        line_end: Some(line_number),
    });
    header.notes.remove(note_idx);

    let header_json = serde_json::to_string_pretty(&header).expect("header serialization");
    let deleted_key = format!("NOTE:{}", note_id);

    let lines: Vec<&str> = raw.lines().collect();
    let delimiters: Vec<(usize, String, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_delimiter(line).map(|(uuid, key)| (i, uuid, key)))
        .collect();

    let mut new_file = header_json;
    new_file.push('\n');
    for (idx, (line_idx, _, key)) in delimiters.iter().enumerate() {
        if key == &deleted_key {
            continue;
        }
        let start = *line_idx;
        let end = if idx + 1 < delimiters.len() {
            delimiters[idx + 1].0
        } else {
            lines.len()
        };
        new_file.push_str(&lines[start..end].join("\n"));
        new_file.push('\n');
    }
    new_file.push_str(&delimiter(&file_id, &new_sha1));
    new_file.push('\n');
    new_file.push_str(&new_line);

    Ok(new_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MD: &str = "# Chapter One\n\nHello world.\n\nSecond paragraph with blackhole here. Another blackhole follows.";

    #[test]
    fn create_produces_parseable_header() {
        let id = "test-uuid-1234";
        let raw = create(id, "Chapter One", Some("A test chapter."), SAMPLE_MD);
        assert!(raw.starts_with('{'));
        assert!(raw.contains(&format!("\"id\": \"{}\"", id)));
        assert!(raw.contains("======!"));
    }

    #[test]
    fn reconstruct_recovers_original_content() {
        let raw = create("test-id", "Chapter One", None, SAMPLE_MD);
        let result = reconstruct(&raw).unwrap();
        assert_eq!(result, SAMPLE_MD);
    }

    #[test]
    fn reconstruct_errors_on_missing_delimiter() {
        assert!(reconstruct("just some json without a delimiter").is_err());
    }

    #[test]
    fn sha1_is_stable() {
        assert_eq!(sha1_hex("hello"), sha1_hex("hello"));
        assert_ne!(sha1_hex("hello"), sha1_hex("world"));
    }

    #[test]
    fn read_returns_empty_notes_for_fresh_page() {
        let raw = create("ch-01", "Chapter One", None, SAMPLE_MD);
        let page = read(&raw).unwrap();
        assert_eq!(page.content, SAMPLE_MD);
        assert!(page.notes.is_empty());
    }

    #[test]
    fn add_note_assigns_id_1_on_first_note() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (_, note) = add_note(&raw, NoteType::Definition, "blackhole", 1, "A region of spacetime.").unwrap();
        assert_eq!(note.id, 1);
        assert_eq!(note.word, "blackhole");
    }

    #[test]
    fn add_note_inserts_anchor_after_target_word() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "blackhole", 1, "Definition body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "The blackhole[^*1] is here.");
    }

    #[test]
    fn add_note_respects_occurrence_index() {
        let raw = create("ch-01", "Chapter", None, "First blackhole. Second blackhole here.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "blackhole", 2, "Body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "First blackhole. Second blackhole[^*1] here.");
    }

    #[test]
    fn add_note_increments_ids_across_calls() {
        let raw = create("ch-01", "Chapter", None, "First blackhole. Second blackhole here.");
        let (raw, n1) =
            add_note(&raw, NoteType::Definition, "blackhole", 1, "A").unwrap();
        let (raw, n2) =
            add_note(&raw, NoteType::Definition, "blackhole", 2, "B").unwrap();
        assert_eq!(n1.id, 1);
        assert_eq!(n2.id, 2);
        let content = reconstruct(&raw).unwrap();
        assert_eq!(content, "First blackhole[^*1]. Second blackhole[^*2] here.");
    }

    #[test]
    fn add_note_persists_body_in_note_block() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "blackhole", 1, "Definition body text").unwrap();
        assert!(new_raw.contains("======! ch-01|NOTE:1 !======"));
        assert!(new_raw.contains("Definition body text"));
    }

    #[test]
    fn add_note_returns_error_for_missing_word() {
        let raw = create("ch-01", "Chapter", None, "Just some text.");
        let result = add_note(&raw, NoteType::Definition, "blackhole", 1, "Body");
        assert!(result.is_err());
    }

    #[test]
    fn add_note_returns_error_for_out_of_range_occurrence() {
        let raw = create("ch-01", "Chapter", None, "One blackhole only.");
        let result = add_note(&raw, NoteType::Definition, "blackhole", 2, "Body");
        assert!(result.is_err());
    }

    #[test]
    fn add_note_finds_target_across_bold_markers() {
        // Source has `**important** fact`; the user's rendered selection
        // is the plain "important fact". The anchor should land right after
        // the closing `**`.
        let raw = create("ch-01", "Chapter", None, "An **important** fact about science.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "important fact", 1, "Body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(
            content,
            "An **important** fact[^*1] about science."
        );
    }

    #[test]
    fn add_note_finds_target_inside_italic_markers() {
        let raw = create("ch-01", "Chapter", None, "The _energy_ flows through cells.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "energy", 1, "Body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "The _energy_[^*1] flows through cells.");
    }

    #[test]
    fn add_note_finds_target_with_inline_code() {
        let raw = create("ch-01", "Chapter", None, "Call `printf` to print.");
        let (new_raw, _) =
            add_note(&raw, NoteType::Definition, "printf", 1, "Body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "Call `printf`[^*1] to print.");
    }

    #[test]
    fn add_note_respects_word_boundaries() {
        // "ole" matches inside "blackhole" but with surrounding word chars — should be skipped.
        let raw = create("ch-01", "Chapter", None, "The blackhole has ole here.");
        let (new_raw, _) = add_note(&raw, NoteType::Definition, "ole", 1, "Body").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "The blackhole has ole[^*1] here.");
    }

    #[test]
    fn delete_note_removes_anchor_meta_and_block() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (raw, _) =
            add_note(&raw, NoteType::Definition, "blackhole", 1, "Definition body").unwrap();
        let raw = delete_note(&raw, 1).unwrap();

        let content = reconstruct(&raw).unwrap();
        assert_eq!(content, "The blackhole is here.");

        let page = read(&raw).unwrap();
        assert!(page.notes.is_empty());

        assert!(!raw.contains("======! ch-01|NOTE:1 !======"));
        assert!(!raw.contains("Definition body"));
    }

    #[test]
    fn delete_note_preserves_other_notes() {
        let raw = create("ch-01", "Chapter", None, "First blackhole. Second blackhole here.");
        let (raw, _) = add_note(&raw, NoteType::Definition, "blackhole", 1, "First def").unwrap();
        let (raw, _) = add_note(&raw, NoteType::Definition, "blackhole", 2, "Second def").unwrap();
        let raw = delete_note(&raw, 1).unwrap();

        let page = read(&raw).unwrap();
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].id, 2);
        assert_eq!(page.notes[0].body, "Second def");
        assert_eq!(page.content, "First blackhole. Second blackhole[^*2] here.");
    }

    #[test]
    fn delete_note_does_not_reuse_id() {
        let raw = create("ch-01", "Chapter", None, "A blackhole and a blackhole.");
        let (raw, _) = add_note(&raw, NoteType::Definition, "blackhole", 1, "A").unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        let (_, note) = add_note(&raw, NoteType::Definition, "blackhole", 1, "B").unwrap();
        assert_eq!(note.id, 2, "deleted id 1 should not be reused");
    }

    #[test]
    fn add_note_footnote_uses_dagger_marker() {
        let raw = create("ch-01", "Chapter", None, "The gravitational collapse is fast.");
        let (new_raw, note) = add_note(
            &raw,
            NoteType::Footnote,
            "gravitational collapse",
            1,
            "A few sentences about collapse.",
        )
        .unwrap();
        assert_eq!(note.note_type, NoteType::Footnote);
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "The gravitational collapse[^†1] is fast.");
    }

    #[test]
    fn add_note_supports_mixed_types_with_distinct_markers() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (raw, _) = add_note(&raw, NoteType::Definition, "blackhole", 1, "Def body").unwrap();
        let (raw, _) = add_note(&raw, NoteType::Footnote, "blackhole", 1, "Footnote body").unwrap();
        let content = reconstruct(&raw).unwrap();
        // The definition's anchor is part of the word now; the footnote
        // search must still find "blackhole" at occurrence 1 in the edited
        // body and add its anchor right after.
        assert!(content.contains("[^*1]"));
        assert!(content.contains("[^†2]"));
    }

    #[test]
    fn rewrite_passage_wraps_replacement_in_span() {
        let raw = create("ch-01", "Chapter", None, "The collapse is dramatic.");
        let (new_raw, id) =
            rewrite_passage(&raw, "collapse is dramatic", 1, "fall is sudden and total").unwrap();
        assert_eq!(id, 1);
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(
            content,
            r#"The <span data-rewrite-id="1">fall is sudden and total</span>."#
        );
    }

    #[test]
    fn rewrite_passage_increments_id_across_calls() {
        let raw = create("ch-01", "Chapter", None, "First passage here. Second passage here.");
        let (raw, id1) = rewrite_passage(&raw, "First passage", 1, "Initial bit").unwrap();
        let (_, id2) = rewrite_passage(&raw, "Second passage", 1, "Following bit").unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn rewrite_passage_html_escapes_replacement() {
        let raw = create("ch-01", "Chapter", None, "See the demo here.");
        let (new_raw, _) =
            rewrite_passage(&raw, "demo", 1, "X < Y & Z > W").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert!(content.contains("X &lt; Y &amp; Z &gt; W"));
        assert!(!content.contains("X < Y"));
    }

    #[test]
    fn rewrite_passage_rejects_multi_line_selection() {
        let raw = create("ch-01", "Chapter", None, "A line.\nAnother line.");
        let result = rewrite_passage(&raw, "A line.\nAnother", 1, "X");
        assert!(result.is_err());
    }

    #[test]
    fn rewrite_passage_errors_when_selection_not_found() {
        let raw = create("ch-01", "Chapter", None, "Just some text.");
        assert!(rewrite_passage(&raw, "missing phrase", 1, "X").is_err());
    }

    #[test]
    fn rewrite_existing_span_replaces_with_new_id() {
        let raw = create("ch-01", "Chapter", None, "The collapse is dramatic.");
        let (raw, old_id) =
            rewrite_passage(&raw, "collapse is dramatic", 1, "fall is sudden").unwrap();
        let (new_raw, new_id) =
            rewrite_existing_span(&raw, old_id, "drop is rapid").unwrap();
        assert!(new_id > old_id);
        let content = reconstruct(&new_raw).unwrap();
        assert!(content.contains(&format!(r#"<span data-rewrite-id="{}">drop is rapid</span>"#, new_id)));
        assert!(!content.contains(&format!(r#"data-rewrite-id="{}""#, old_id)));
    }

    #[test]
    fn rewrite_existing_span_html_escapes_content() {
        let raw = create("ch-01", "Chapter", None, "The collapse here.");
        let (raw, old_id) = rewrite_passage(&raw, "collapse", 1, "first").unwrap();
        let (new_raw, _) = rewrite_existing_span(&raw, old_id, "A < B").unwrap();
        assert!(reconstruct(&new_raw).unwrap().contains("A &lt; B"));
    }

    #[test]
    fn rewrite_existing_span_errors_for_unknown_id() {
        let raw = create("ch-01", "Chapter", None, "Just plain text.");
        assert!(rewrite_existing_span(&raw, 99, "X").is_err());
    }

    #[test]
    fn rewrite_passage_id_never_reused_after_future_features() {
        // The next_rewrite_id counter advances even if a span is later replaced
        // (replacement logic isn't here yet, but the counter must be stable).
        let raw = create("ch-01", "Chapter", None, "A B C D E F.");
        let (raw, id1) = rewrite_passage(&raw, "A B C", 1, "ABC").unwrap();
        let (_, id2) = rewrite_passage(&raw, "D E F", 1, "DEF").unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    const SVG: &str = r#"<svg viewBox="0 0 100 50"><rect width="100" height="50"/></svg>"#;

    #[test]
    fn add_artifact_wide_places_image_on_its_own_paragraph() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (new_raw, art) =
            add_artifact(&raw, "collapse", 1, SVG, "A collapsing star", 2.0, "image").unwrap();
        assert_eq!(art.id, 1);
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(
            content,
            "The collapse is shown here.\n\n![A collapsing star](epar://1)"
        );
    }

    #[test]
    fn add_artifact_tall_places_image_inline() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (new_raw, _) =
            add_artifact(&raw, "collapse", 1, SVG, "Tall image", 0.5, "image").unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(
            content,
            "The collapse ![Tall image](epar://1) is shown here."
        );
    }

    #[test]
    fn add_artifact_stores_svg_and_metadata() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (new_raw, art) =
            add_artifact(&raw, "collapse", 1, SVG, "A collapsing star", 2.0, "image").unwrap();
        assert!(new_raw.contains("======! ch-01|ARTIFACT:1 !======"));
        assert!(new_raw.contains(SVG));
        assert_eq!(art.mime_type, "image/svg+xml");
        assert_eq!(art.semantic_type, "image");
        assert_eq!(art.aspect_ratio, 2.0);
        assert_eq!(art.source, "collapse");

        let page = read(&new_raw).unwrap();
        assert_eq!(page.artifacts.len(), 1);
        assert_eq!(page.artifacts[0].body, SVG);
        assert_eq!(page.artifacts[0].caption.as_deref(), Some("A collapsing star"));
    }

    #[test]
    fn add_artifact_increments_id() {
        let raw = create("ch-01", "Chapter", None, "First spot and second spot here.");
        let (raw, a1) = add_artifact(&raw, "First spot", 1, SVG, "one", 2.0, "image").unwrap();
        let (_, a2) = add_artifact(&raw, "second spot", 1, SVG, "two", 2.0, "image").unwrap();
        assert_eq!(a1.id, 1);
        assert_eq!(a2.id, 2);
    }

    #[test]
    fn add_artifact_sanitizes_alt_text() {
        let raw = create("ch-01", "Chapter", None, "The collapse here.");
        let (new_raw, _) =
            add_artifact(&raw, "collapse", 1, SVG, "weird [brackets] (parens)", 2.0, "image")
                .unwrap();
        let content = reconstruct(&new_raw).unwrap();
        // Alt text has brackets/parens stripped so markdown stays valid.
        assert!(content.contains("![weird brackets parens](epar://1)"));
        // But the stored caption keeps the original text.
        let page = read(&new_raw).unwrap();
        assert_eq!(
            page.artifacts[0].caption.as_deref(),
            Some("weird [brackets] (parens)")
        );
    }

    #[test]
    fn add_artifact_errors_when_selection_missing() {
        let raw = create("ch-01", "Chapter", None, "Nothing relevant here.");
        assert!(add_artifact(&raw, "absent phrase", 1, SVG, "x", 2.0, "image").is_err());
    }

    #[test]
    fn delete_artifact_removes_block_image_cleanly() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (raw, _) = add_artifact(&raw, "collapse", 1, SVG, "cap", 2.0, "image").unwrap();
        let raw = delete_artifact(&raw, 1).unwrap();
        assert_eq!(reconstruct(&raw).unwrap(), "The collapse is shown here.");
        assert!(read(&raw).unwrap().artifacts.is_empty());
        assert!(!raw.contains("ARTIFACT:1"));
        assert!(!raw.contains(SVG));
    }

    #[test]
    fn delete_artifact_removes_inline_image_cleanly() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (raw, _) = add_artifact(&raw, "collapse", 1, SVG, "cap", 0.5, "image").unwrap();
        // Sanity: it was inserted inline.
        assert_eq!(
            reconstruct(&raw).unwrap(),
            "The collapse ![cap](epar://1) is shown here."
        );
        let raw = delete_artifact(&raw, 1).unwrap();
        assert_eq!(reconstruct(&raw).unwrap(), "The collapse is shown here.");
        assert!(read(&raw).unwrap().artifacts.is_empty());
    }

    #[test]
    fn delete_artifact_preserves_other_artifacts() {
        let raw = create("ch-01", "Chapter", None, "First spot and second spot here.");
        let (raw, _) = add_artifact(&raw, "First spot", 1, SVG, "one", 0.5, "image").unwrap();
        let (raw, _) = add_artifact(&raw, "second spot", 1, SVG, "two", 0.5, "image").unwrap();
        let raw = delete_artifact(&raw, 1).unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.artifacts.len(), 1);
        assert_eq!(page.artifacts[0].id, 2);
        assert!(page.content.contains("![two](epar://2)"));
        assert!(!page.content.contains("epar://1"));
    }

    #[test]
    fn delete_artifact_errors_for_unknown_id() {
        let raw = create("ch-01", "Chapter", None, "Nothing here.");
        assert!(delete_artifact(&raw, 99).is_err());
    }

    #[test]
    fn regenerate_artifact_swaps_svg_and_metadata_without_moving() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (raw, _) = add_artifact(&raw, "collapse", 1, SVG, "cap", 2.0, "image").unwrap();
        let new_svg = r#"<svg viewBox="0 0 100 40"><circle/></svg>"#;
        let (raw, meta) = regenerate_artifact(&raw, 1, new_svg, "new cap", 2.5).unwrap();
        assert_eq!(meta.aspect_ratio, 2.5);
        let page = read(&raw).unwrap();
        assert_eq!(page.artifacts[0].body, new_svg);
        assert_eq!(page.artifacts[0].caption.as_deref(), Some("new cap"));
        // Still wide → still a block paragraph, body marker unchanged.
        assert_eq!(page.content, "The collapse is shown here.\n\n![cap](epar://1)");
    }

    #[test]
    fn regenerate_artifact_moves_block_to_inline_when_now_tall() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (raw, _) = add_artifact(&raw, "collapse", 1, SVG, "cap", 2.0, "image").unwrap();
        let (raw, _) = regenerate_artifact(&raw, 1, SVG, "tall", 0.5).unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.artifacts[0].aspect_ratio, 0.5);
        assert_eq!(page.content, "The collapse is shown here. ![tall](epar://1)");
    }

    #[test]
    fn regenerate_artifact_moves_inline_to_block_when_now_wide() {
        let raw = create("ch-01", "Chapter", None, "The collapse is shown here.");
        let (raw, _) = add_artifact(&raw, "collapse", 1, SVG, "cap", 0.5, "image").unwrap();
        let (raw, _) = regenerate_artifact(&raw, 1, SVG, "wide", 2.0).unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.artifacts[0].aspect_ratio, 2.0);
        assert_eq!(page.content, "The collapse is shown here.\n\n![wide](epar://1)");
    }

    #[test]
    fn regenerate_artifact_errors_for_unknown_id() {
        let raw = create("ch-01", "Chapter", None, "Nothing here.");
        assert!(regenerate_artifact(&raw, 99, SVG, "x", 1.0).is_err());
    }

    #[test]
    fn insert_appendix_ref_adds_marker_after_target() {
        let raw = create("ch-01", "Chapter", None, "See the gravitational collapse here.");
        let new_raw =
            insert_appendix_ref(&raw, 1, "gravitational collapse", 1).unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "See the gravitational collapse[^A1] here.");
    }

    #[test]
    fn insert_appendix_ref_uses_given_sequence_number() {
        let raw = create("ch-01", "Chapter", None, "See the collapse here.");
        let new_raw = insert_appendix_ref(&raw, 4, "collapse", 1).unwrap();
        let content = reconstruct(&new_raw).unwrap();
        assert!(content.contains("[^A4]"));
    }

    #[test]
    fn insert_appendix_ref_does_not_add_note_metadata() {
        let raw = create("ch-01", "Chapter", None, "See the collapse here.");
        let new_raw = insert_appendix_ref(&raw, 1, "collapse", 1).unwrap();
        let page = read(&new_raw).unwrap();
        assert!(page.notes.is_empty());
    }

    #[test]
    fn add_note_endnote_uses_double_dagger_marker() {
        let raw = create("ch-01", "Chapter", None, "The Hawking radiation is subtle.");
        let (new_raw, note) = add_note(
            &raw,
            NoteType::Endnote,
            "Hawking radiation",
            1,
            "An endnote explaining Hawking radiation in some depth.",
        )
        .unwrap();
        assert_eq!(note.note_type, NoteType::Endnote);
        let content = reconstruct(&new_raw).unwrap();
        assert_eq!(content, "The Hawking radiation[^‡1] is subtle.");
    }

    #[test]
    fn delete_note_removes_endnote_anchor() {
        let raw = create("ch-01", "Chapter", None, "Note the Hawking radiation here.");
        let (raw, _) =
            add_note(&raw, NoteType::Endnote, "Hawking radiation", 1, "Endnote body").unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        assert_eq!(
            reconstruct(&raw).unwrap(),
            "Note the Hawking radiation here."
        );
        assert!(read(&raw).unwrap().notes.is_empty());
    }

    #[test]
    fn delete_note_removes_footnote_dagger_anchor() {
        let raw = create("ch-01", "Chapter", None, "Look at the gravitational collapse here.");
        let (raw, _) = add_note(
            &raw,
            NoteType::Footnote,
            "gravitational collapse",
            1,
            "Footnote body",
        )
        .unwrap();
        let raw = delete_note(&raw, 1).unwrap();
        let content = reconstruct(&raw).unwrap();
        assert_eq!(content, "Look at the gravitational collapse here.");
        let page = read(&raw).unwrap();
        assert!(page.notes.is_empty());
    }

    #[test]
    fn delete_note_returns_error_for_unknown_id() {
        let raw = create("ch-01", "Chapter", None, "Nothing here.");
        assert!(delete_note(&raw, 99).is_err());
    }

    #[test]
    fn read_returns_notes_with_bodies_after_add() {
        let raw = create("ch-01", "Chapter", None, "The blackhole is here.");
        let (raw, _) =
            add_note(&raw, NoteType::Definition, "blackhole", 1, "Definition body").unwrap();
        let page = read(&raw).unwrap();
        assert_eq!(page.notes.len(), 1);
        assert_eq!(page.notes[0].id, 1);
        assert_eq!(page.notes[0].word, "blackhole");
        assert_eq!(page.notes[0].body, "Definition body");
        assert_eq!(page.content, "The blackhole[^*1] is here.");
    }
}
