use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub metadata: Metadata,
    pub lesson_plan: LessonPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub title: String,
    pub topic: String,
    pub created: String,
    pub modified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reading_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prior_knowledge: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonPlan {
    pub summary: String,
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub title: String,
    pub description: String,
    pub file: String,
    pub status: ChapterStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChapterStatus {
    Planned,
    Generating,
    Generated,
}

pub fn load(path: &Path) -> Result<Manifest, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str(&content).map_err(|e| format!("failed to parse manifest: {e}"))
}

pub fn save(manifest: &Manifest, path: &Path) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(manifest).map_err(|e| format!("failed to serialize: {e}"))?;
    std::fs::write(path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            version: 1,
            metadata: Metadata {
                title: "Black Holes".to_string(),
                topic: "How black holes form".to_string(),
                created: "2026-05-13T00:00:00Z".to_string(),
                modified: "2026-05-13T00:00:00Z".to_string(),
                reading_level: None,
                prior_knowledge: None,
            },
            lesson_plan: LessonPlan {
                summary: "An overview.".to_string(),
                chapters: vec![Chapter {
                    id: "ch-01".to_string(),
                    title: "Stellar Evolution".to_string(),
                    description: "How stars live and die.".to_string(),
                    file: "chapters/01-stellar-evolution.md".to_string(),
                    status: ChapterStatus::Planned,
                }],
            },
        }
    }

    #[test]
    fn roundtrip_serialization() {
        let original = sample();
        let json = serde_json::to_string(&original).unwrap();
        let parsed: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.metadata.title, "Black Holes");
        assert_eq!(parsed.lesson_plan.chapters.len(), 1);
        assert_eq!(parsed.lesson_plan.chapters[0].status, ChapterStatus::Planned);
    }

    #[test]
    fn chapter_status_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&ChapterStatus::Generating).unwrap(), "\"generating\"");
        assert_eq!(serde_json::to_string(&ChapterStatus::Generated).unwrap(), "\"generated\"");
        assert_eq!(serde_json::to_string(&ChapterStatus::Planned).unwrap(), "\"planned\"");
    }

    #[test]
    fn optional_fields_omitted_when_none() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(!json.contains("readingLevel"));
        assert!(!json.contains("priorKnowledge"));
    }

    #[test]
    fn field_names_are_camel_case() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("lessonPlan"));
        assert!(!json.contains("lesson_plan"));
    }

    #[test]
    fn load_save_roundtrip() {
        let path = std::env::temp_dir().join("edu-harness-test.edubook");
        let original = sample();
        save(&original, &path).unwrap();
        let loaded = load(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded.version, original.version);
        assert_eq!(loaded.metadata.topic, original.metadata.topic);
        assert_eq!(loaded.lesson_plan.chapters[0].id, "ch-01");
    }
}
