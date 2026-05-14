use crate::llm::LlmMessage;
use crate::manifest::{Chapter, ChapterStatus, LessonPlan};
use serde::{Deserialize, Serialize};

const SYSTEM_PROMPT: &str = "\
You are helping a user create a personalized educational book. \
Your goal is to gather enough information to tailor the content well. \
Have a brief, friendly conversation — no more than 4-5 exchanges total. \
Explore: any specific aspects they want to focus on (only if the topic is broad), \
their current familiarity with the subject, and relevant background knowledge they have. \
Ask one or two questions per message. Keep responses concise. \
When you have enough to work with, end your final message with exactly: \
\"Ready to generate your lesson plan.\"";

const PLAN_PROMPT: &str = "\
Based on our conversation, generate a lesson plan for the book. \
Output ONLY a valid JSON object — no markdown, no code fences, no explanation. \
Use this exact structure:\n\
{\n  \
  \"summary\": \"2-3 sentence overview of the book's contents\",\n  \
  \"description\": \"2-3 sentence back-cover style description written to entice a reader\",\n  \
  \"readingLevel\": \"beginner|intermediate|advanced\",\n  \
  \"priorKnowledge\": \"brief description of the assumed background\",\n  \
  \"chapters\": [\n    \
    { \"title\": \"Chapter title\", \"description\": \"One sentence description\" }\n  \
  ]\n\
}\n\
Include 6-10 chapters that build knowledge progressively.";

pub fn build_initial_messages(topic: &str) -> Vec<LlmMessage> {
    vec![
        LlmMessage { role: "system".to_string(), content: SYSTEM_PROMPT.to_string() },
        LlmMessage { role: "user".to_string(), content: format!("I want to learn about: {topic}") },
    ]
}

pub fn build_continuation_messages(topic: &str, conversation: Vec<LlmMessage>) -> Vec<LlmMessage> {
    let mut messages = build_initial_messages(topic);
    messages.extend(conversation);
    messages
}

pub fn build_plan_messages(topic: &str, conversation: Vec<LlmMessage>) -> Vec<LlmMessage> {
    let mut messages = build_continuation_messages(topic, conversation);
    messages.push(LlmMessage { role: "user".to_string(), content: PLAN_PROMPT.to_string() });
    messages
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedPlan {
    pub summary: String,
    pub description: String,
    pub reading_level: String,
    pub prior_knowledge: String,
    pub lesson_plan: LessonPlan,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    summary: String,
    description: String,
    #[serde(rename = "readingLevel")]
    reading_level: String,
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
            Chapter {
                id: format!("ch-{n:02}"),
                file: format!("chapters/{n:02}-{}.md", slugify(&ch.title)),
                title: ch.title,
                description: ch.description,
                status: ChapterStatus::Planned,
            }
        })
        .collect();

    Ok(GeneratedPlan {
        reading_level: raw.reading_level,
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
            "readingLevel": "intermediate",
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
        assert_eq!(plan.reading_level, "intermediate");
        assert_eq!(plan.description, "Explore the universe's most extreme objects.");
        assert_eq!(plan.lesson_plan.chapters.len(), 2);
        assert_eq!(plan.lesson_plan.chapters[0].id, "ch-01");
        assert_eq!(plan.lesson_plan.chapters[0].file, "chapters/01-stellar-evolution.md");
    }

    #[test]
    fn parse_plan_strips_code_fences() {
        let wrapped = "```json\n{\"summary\":\"s\",\"description\":\"d\",\"readingLevel\":\"beginner\",\"priorKnowledge\":\"none\",\"chapters\":[]}\n```";
        let plan = parse_plan(wrapped).unwrap();
        assert_eq!(plan.reading_level, "beginner");
    }

    #[test]
    fn parse_plan_extracts_json_from_prose() {
        let messy = "Here is the plan:\n{\"summary\":\"s\",\"description\":\"d\",\"readingLevel\":\"advanced\",\"priorKnowledge\":\"lots\",\"chapters\":[]}\nDone.";
        let plan = parse_plan(messy).unwrap();
        assert_eq!(plan.reading_level, "advanced");
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
        let messages = build_continuation_messages("black holes", convo);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        assert!(messages[1].content.contains("black holes"));
        assert_eq!(messages[2].role, "assistant");
        assert_eq!(messages.len(), 4);
    }
}
