use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

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

fn sha1_hex(content: &str) -> String {
    let mut h = Sha1::new();
    h.update(content.as_bytes());
    format!("{:x}", h.finalize())
}

fn delimiter(file_id: &str, sha1: &str) -> String {
    format!("======! {}|{} !======", file_id, sha1)
}

fn parse_delimiter(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix("======!")?.strip_suffix("!======")?;
    let inner = inner.trim();
    inner
        .split_once('|')
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
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
    };

    let header_json = serde_json::to_string_pretty(&header).expect("edupage header serialization");
    format!("{}\n{}\n{}", header_json, delimiter(file_id, &sha1), content)
}

pub fn reconstruct(raw: &str) -> Result<String, String> {
    let lines: Vec<&str> = raw.lines().collect();

    let delimiters: Vec<(usize, String, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, line)| parse_delimiter(line).map(|(uuid, sha1)| (i, uuid, sha1)))
        .collect();

    if delimiters.is_empty() {
        return Err("no revision delimiters found in edupage".to_string());
    }

    let header_end = delimiters[0].0;
    let header: EdupageHeader = serde_json::from_str(&lines[..header_end].join("\n"))
        .map_err(|e| format!("invalid edupage header: {e}"))?;

    let mut blocks: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (idx, (line_idx, _, sha1)) in delimiters.iter().enumerate() {
        let start = line_idx + 1;
        let end = if idx + 1 < delimiters.len() { delimiters[idx + 1].0 } else { lines.len() };
        blocks.insert(sha1.clone(), lines[start..end].join("\n"));
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MD: &str = "# Chapter One\n\nHello world.\n\nSecond paragraph.";

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
}
