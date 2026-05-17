use crate::llm::LlmMessage;
use crate::manifest::{Chapter, ChapterStatus, LessonPlan};
use serde::{Deserialize, Serialize};

fn reading_level_description(level: &str) -> &'static str {
    match level {
        "child" => "a child (ages 8–12)",
        "teen" => "a teen (ages 13–17)",
        "academic" => "a professional or postgraduate",
        _ => "a general adult",
    }
}

fn build_system_prompt(reading_level: &str) -> String {
    format!(
        "You are helping a user create a personalized educational book. \
         Your goal is to gather enough information to tailor the content well. \
         The reader has selected a {} reading level — pitch your language and questions at that level throughout. \
         Have a brief, friendly conversation — no more than 4-5 exchanges total. \
         Explore: any specific aspects they want to focus on (only if the topic is broad), \
         their current familiarity with the subject, and relevant background knowledge they have. \
         Ask one or two questions per message. Keep responses concise. \
         When you have enough to work with, end your final message with exactly: \
         \"Ready to generate your lesson plan.\"",
        reading_level_description(reading_level)
    )
}

fn build_plan_prompt(reading_level: &str) -> String {
    format!(
        "Based on our conversation, generate a lesson plan for the book. \
         Write all summaries and descriptions appropriate for {} reading level. \
         Output ONLY a valid JSON object — no markdown, no code fences, no explanation. \
         Use this exact structure:\n\
         {{\n  \
           \"summary\": \"2-3 sentence overview of the book's contents\",\n  \
           \"description\": \"2-3 sentence back-cover style description written to entice a reader\",\n  \
           \"priorKnowledge\": \"brief description of the assumed background\",\n  \
           \"chapters\": [\n    \
             {{ \"title\": \"Chapter title\", \"description\": \"One sentence description\" }}\n  \
           ]\n\
         }}\n\
         Include 6-10 chapters that build knowledge progressively.",
        reading_level_description(reading_level)
    )
}

pub fn build_initial_messages(topic: &str, reading_level: &str) -> Vec<LlmMessage> {
    vec![
        LlmMessage { role: "system".to_string(), content: build_system_prompt(reading_level) },
        LlmMessage { role: "user".to_string(), content: format!("I want to learn about: {topic}") },
    ]
}

pub fn build_continuation_messages(topic: &str, reading_level: &str, conversation: Vec<LlmMessage>) -> Vec<LlmMessage> {
    let mut messages = build_initial_messages(topic, reading_level);
    messages.extend(conversation);
    messages
}

pub fn build_plan_messages(topic: &str, reading_level: &str, conversation: Vec<LlmMessage>) -> Vec<LlmMessage> {
    let mut messages = build_continuation_messages(topic, reading_level, conversation);
    messages.push(LlmMessage { role: "user".to_string(), content: build_plan_prompt(reading_level) });
    messages
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPlan {
    pub summary: String,
    pub description: String,
    pub prior_knowledge: String,
    pub lesson_plan: LessonPlan,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    summary: String,
    description: String,
    #[serde(rename = "priorKnowledge")]
    prior_knowledge: String,
    chapters: Vec<RawChapter>,
}

#[derive(Debug, Deserialize)]
struct RawChapter {
    title: String,
    description: String,
}

pub fn parse_plan(response: &str) -> Result<GeneratedPlan, String> {
    let json = extract_json(response);
    let raw: RawPlan = serde_json::from_str(&json)
        .map_err(|e| format!("failed to parse lesson plan: {e}\n---\n{json}"))?;

    let chapters = raw.chapters
        .into_iter()
        .enumerate()
        .map(|(i, ch)| {
            let n = i + 1;
            let id = uuid::Uuid::new_v4().to_string();
            Chapter {
                id,
                file: format!("chapters/{n:02}-{}.edupage", slugify(&ch.title)),
                title: ch.title,
                description: Some(ch.description),
                status: ChapterStatus::Planned,
            }
        })
        .collect();

    Ok(GeneratedPlan {
        prior_knowledge: raw.prior_knowledge,
        description: raw.description,
        lesson_plan: LessonPlan { summary: raw.summary.clone(), chapters },
        summary: raw.summary,
    })
}

fn extract_json(s: &str) -> String {
    let s = s.trim();
    // Strip ```json ... ``` or ``` ... ```
    if let Some(inner) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")) {
        if let Some(inner) = inner.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    // Extract first complete { ... } block as a fallback
    if let (Some(start), Some(end)) = (s.find('{'), s.rfind('}')) {
        return s[start..=end].to_string();
    }
    s.to_string()
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_json(extra: &str) -> String {
        format!(r#"{{
            "summary": "A book about black holes.",
            "description": "Explore the universe's most extreme objects.",
            "priorKnowledge": "Basic physics",
            {extra}
            "chapters": [
                {{"title": "Stellar Evolution", "description": "How stars live and die."}},
                {{"title": "Gravitational Collapse", "description": "The mechanics of collapse."}}
            ]
        }}"#)
    }

    #[test]
    fn parse_plan_valid_json() {
        let plan = parse_plan(&plan_json("")).unwrap();
        assert_eq!(plan.prior_knowledge, "Basic physics");
        assert_eq!(plan.description, "Explore the universe's most extreme objects.");
        assert_eq!(plan.lesson_plan.chapters.len(), 2);
        assert!(!plan.lesson_plan.chapters[0].id.is_empty());
        assert!(plan.lesson_plan.chapters[0].file.ends_with(".edupage"));
        assert!(plan.lesson_plan.chapters[0].file.contains("stellar-evolution"));
    }

    #[test]
    fn parse_plan_strips_code_fences() {
        let wrapped = "```json\n{\"summary\":\"s\",\"description\":\"d\",\"priorKnowledge\":\"none\",\"chapters\":[]}\n```";
        let plan = parse_plan(wrapped).unwrap();
        assert_eq!(plan.prior_knowledge, "none");
    }

    #[test]
    fn parse_plan_extracts_json_from_prose() {
        let messy = "Here is the plan:\n{\"summary\":\"s\",\"description\":\"d\",\"priorKnowledge\":\"lots\",\"chapters\":[]}\nDone.";
        let plan = parse_plan(messy).unwrap();
        assert_eq!(plan.prior_knowledge, "lots");
    }

    #[test]
    fn slugify_produces_clean_filenames() {
        assert_eq!(slugify("Stellar Evolution"), "stellar-evolution");
        assert_eq!(slugify("What is a Black Hole?"), "what-is-a-black-hole");
        assert_eq!(slugify("E=mc²"), "e-mc");
    }

    #[test]
    fn continuation_messages_prepend_context() {
        let convo = vec![
            LlmMessage { role: "assistant".to_string(), content: "What do you know?".to_string() },
            LlmMessage { role: "user".to_string(), content: "Not much.".to_string() },
        ];
        let messages = build_continuation_messages("black holes", "adult", convo);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert!(messages[1].content.contains("black holes"));
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages.len(), 4);
    }

    #[test]
    fn system_prompt_includes_reading_level() {
        let cases = [
            ("child", "child (ages 8–12)"),
            ("teen", "teen (ages 13–17)"),
            ("adult", "general adult"),
            ("academic", "professional or postgraduate"),
        ];
        for (level, expected) in cases {
            let msgs = build_initial_messages("black holes", level);
            assert!(
                msgs[0].content.contains(expected),
                "system prompt for level '{level}' should contain '{expected}'"
            );
        }
    }

    #[test]
    fn plan_prompt_includes_reading_level() {
        let cases = [
            ("child", "child (ages 8–12)"),
            ("teen", "teen (ages 13–17)"),
            ("adult", "general adult"),
            ("academic", "professional or postgraduate"),
        ];
        for (level, expected) in cases {
            let msgs = build_plan_messages("black holes", level, vec![]);
            let plan_msg = msgs.last().unwrap();
            assert_eq!(plan_msg.role, "user");
            assert!(
                plan_msg.content.contains(expected),
                "plan prompt for level '{level}' should contain '{expected}'"
            );
        }
    }
}
