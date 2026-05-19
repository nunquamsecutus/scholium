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
}

fn default_next_note_id() -> u32 {
    1
}

fn default_next_rewrite_id() -> u32 {
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

#[derive(Debug, Serialize)]
pub struct EduPage {
    pub content: String,
    pub notes: Vec<NoteWithBody>,
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

    Ok(EduPage { content, notes })
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
        let mut search_start = 0;
        while let Some(rel_pos) = line[search_start..].find(query) {
            let pos = search_start + rel_pos;
            let end = pos + query.len();

            let before_is_word = pos > 0
                && line[..pos]
                    .chars()
                    .last()
                    .map(is_word_char)
                    .unwrap_or(false);
            let after_is_word = end < line.len()
                && line[end..]
                    .chars()
                    .next()
                    .map(is_word_char)
                    .unwrap_or(false);

            if !before_is_word && !after_is_word {
                count += 1;
                if count == target_n {
                    let new_line = format!("{}{}{}", &line[..end], anchor, &line[end..]);
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
        let mut search_start = 0;
        while let Some(rel_pos) = line[search_start..].find(selection) {
            let pos = search_start + rel_pos;
            let end = pos + selection.len();

            let before_is_word = pos > 0
                && line[..pos]
                    .chars()
                    .last()
                    .map(is_word_char)
                    .unwrap_or(false);
            let after_is_word = end < line.len()
                && line[end..]
                    .chars()
                    .next()
                    .map(is_word_char)
                    .unwrap_or(false);

            if !before_is_word && !after_is_word {
                count += 1;
                if count == target_n {
                    let new_line =
                        format!("{}{}{}", &line[..pos], replacement, &line[end..]);
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
