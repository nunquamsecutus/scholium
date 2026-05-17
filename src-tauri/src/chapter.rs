use crate::llm::LlmMessage;
use crate::manifest::Manifest;

pub fn build_messages(manifest: &Manifest, chapter_id: &str) -> Result<Vec<LlmMessage>, String> {
    let meta = &manifest.metadata;
    let plan = &manifest.lesson_plan;

    let chapter = plan
        .chapters
        .iter()
        .find(|ch| ch.id == chapter_id)
        .ok_or_else(|| format!("chapter {chapter_id} not found"))?;

    let chapter_num = plan
        .chapters
        .iter()
        .position(|ch| ch.id == chapter_id)
        .unwrap_or(0)
        + 1;

    let outline = plan
        .chapters
        .iter()
        .enumerate()
        .map(|(i, ch)| match &ch.description {
            Some(desc) => format!("{}. {} — {}", i + 1, ch.title, desc),
            None => format!("{}. {}", i + 1, ch.title),
        })
        .collect::<Vec<_>>()
        .join("\n");

    let system = format!(
        "You are writing a chapter for an educational book titled \"{title}\".\n\n\
         Book overview: {summary}\n\
         Target reader: {level} level, background in {background}\n\n\
         Full chapter outline:\n{outline}\n\n\
         Write in clear, engaging prose suited to the reading level. \
         Use markdown: # for the chapter title, ## and ### for sections, \
         **bold** for key terms introduced for the first time, \
         and fenced code blocks where appropriate. \
         Build naturally on concepts from earlier chapters. \
         Start directly with the chapter title as a # heading — \
         do not include a preamble, chapter number, or table of contents.",
        title = meta.title,
        summary = plan.summary,
        level = meta.reading_level.as_deref().unwrap_or("general"),
        background = meta.prior_knowledge.as_deref().unwrap_or("general knowledge"),
        outline = outline,
    );

    let user = match &chapter.description {
        Some(desc) => format!(
            "Write Chapter {n}: {title}\n\n{desc}\n\n\
             Aim for roughly 1500–2500 words. Be thorough.",
            n = chapter_num,
            title = chapter.title,
        ),
        None => format!(
            "Write Chapter {n}: {title}\n\nAim for roughly 1500–2500 words. Be thorough.",
            n = chapter_num,
            title = chapter.title,
        ),
    };

    Ok(vec![
        LlmMessage { role: "system".to_string(), content: system },
        LlmMessage { role: "user".to_string(), content: user },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Chapter, ChapterStatus, LessonPlan, Manifest, Metadata};

    fn sample_manifest() -> Manifest {
        Manifest {
            version: 1,
            metadata: Metadata {
                title: "Black Holes".to_string(),
                subtitle: None,
                topic: "Black holes".to_string(),
                prompt: "Black holes".to_string(),
                created: "2026-05-14T00:00:00Z".to_string(),
                modified: "2026-05-14T00:00:00Z".to_string(),
                description: None,
                reading_level: Some("intermediate".to_string()),
                prior_knowledge: Some("basic physics".to_string()),
            },
            lesson_plan: LessonPlan {
                summary: "A comprehensive look at black holes.".to_string(),
                chapters: vec![
                    Chapter {
                        id: "ch-01".to_string(),
                        title: "Stellar Evolution".to_string(),
                        description: Some("How stars live and die.".to_string()),
                        file: "chapters/01-stellar-evolution.edupage".to_string(),
                        status: ChapterStatus::Planned,
                    },
                    Chapter {
                        id: "ch-02".to_string(),
                        title: "Gravitational Collapse".to_string(),
                        description: Some("The mechanics of collapse.".to_string()),
                        file: "chapters/02-gravitational-collapse.edupage".to_string(),
                        status: ChapterStatus::Planned,
                    },
                ],
            },
        }
    }

    #[test]
    fn messages_contain_book_context() {
        let manifest = sample_manifest();
        let msgs = build_messages(&manifest, "ch-01").unwrap();
        let system = msgs.iter().find(|m| m.role == "system").unwrap();
        assert!(system.content.contains("Black Holes"));
        assert!(system.content.contains("intermediate"));
        assert!(system.content.contains("basic physics"));
        assert!(system.content.contains("Stellar Evolution"));
        assert!(system.content.contains("Gravitational Collapse"));
    }

    #[test]
    fn user_message_has_correct_chapter_number() {
        let manifest = sample_manifest();
        let msgs = build_messages(&manifest, "ch-02").unwrap();
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        assert!(user.content.contains("Chapter 2"));
        assert!(user.content.contains("Gravitational Collapse"));
    }

    #[test]
    fn returns_error_for_unknown_chapter() {
        let manifest = sample_manifest();
        assert!(build_messages(&manifest, "ch-99").is_err());
    }
}
