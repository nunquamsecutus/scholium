use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

/// An image attached to a vision request (base64-encoded bytes + media type).
pub struct LlmImage {
    pub media_type: String,
    pub base64_data: String,
}

/// Default text model for general work (onboarding, chapters, notes, …).
pub const CLAUDE_DEFAULT_MODEL: &str = "claude-sonnet-4-6";
/// Stronger model for the harder task of authoring SVG illustrations.
pub const CLAUDE_GENERATION_MODEL: &str = "claude-opus-4-7";
/// Vision model used to critique a rendered illustration — judging is easier
/// than generating, so a lighter model suffices.
pub const CLAUDE_CRITIQUE_MODEL: &str = "claude-sonnet-4-6";

// POST a prepared request body to the Claude messages API and return the
// first text block. Shared by the text and vision calls.
async fn post_claude(api_key: &str, body: serde_json::Value) -> Result<String, String> {
    let client = reqwest::Client::new();
    let res = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Claude API {status}: {text}"));
    }

    let json: serde_json::Value = res.json().await.map_err(|e| format!("parse error: {e}"))?;
    json["content"][0]["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "unexpected response shape from Claude".to_string())
}

fn split_system(messages: &[LlmMessage]) -> (Option<String>, Vec<&LlmMessage>) {
    let system = messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());
    let chat = messages.iter().filter(|m| m.role != "system").collect();
    (system, chat)
}

pub async fn call_claude(
    api_key: &str,
    model: &str,
    messages: Vec<LlmMessage>,
) -> Result<String, String> {
    // Claude uses a top-level `system` field rather than a system role in messages.
    let (system, chat_messages) = split_system(&messages);

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 8192,
        "messages": chat_messages,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

    post_claude(api_key, body).await
}

/// Like `call_claude`, but attaches `image` to the final user message as an
/// image content block — used for the SVG render-and-critique refinement.
pub async fn call_claude_vision(
    api_key: &str,
    model: &str,
    messages: Vec<LlmMessage>,
    image: &LlmImage,
) -> Result<String, String> {
    let (system, chat) = split_system(&messages);
    if chat.is_empty() {
        return Err("vision request needs a user message".to_string());
    }

    let last = chat.len() - 1;
    let api_messages: Vec<serde_json::Value> = chat
        .iter()
        .enumerate()
        .map(|(i, m)| {
            if i == last {
                serde_json::json!({
                    "role": m.role,
                    "content": [
                        { "type": "text", "text": m.content },
                        { "type": "image", "source": {
                            "type": "base64",
                            "media_type": image.media_type,
                            "data": image.base64_data,
                        }},
                    ],
                })
            } else {
                serde_json::json!({ "role": m.role, "content": m.content })
            }
        })
        .collect();

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 8192,
        "messages": api_messages,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

    post_claude(api_key, body).await
}

pub async fn call_ollama(url: &str, model: &str, messages: Vec<LlmMessage>) -> Result<String, String> {
    let client = reqwest::Client::new();
    let endpoint = format!("{}/api/chat", url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });

    let res = client
        .post(&endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Ollama {status}: {text}"));
    }

    let json: serde_json::Value = res.json().await.map_err(|e| format!("parse error: {e}"))?;
    json["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "unexpected response shape from Ollama".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llm_message_serializes_correctly() {
        let msg = LlmMessage { role: "user".to_string(), content: "Hello".to_string() };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "Hello");
    }

    #[test]
    fn ollama_url_trailing_slash_is_trimmed() {
        let url = "http://localhost:11434/";
        let endpoint = format!("{}/api/chat", url.trim_end_matches('/'));
        assert_eq!(endpoint, "http://localhost:11434/api/chat");
    }

    #[test]
    fn system_message_is_filtered_for_claude_body() {
        let messages = vec![
            LlmMessage { role: "system".to_string(), content: "You are helpful.".to_string() },
            LlmMessage { role: "user".to_string(), content: "Hello".to_string() },
        ];
        let system = messages.iter().find(|m| m.role == "system").map(|m| m.content.clone());
        let chat: Vec<&LlmMessage> = messages.iter().filter(|m| m.role != "system").collect();
        assert_eq!(system, Some("You are helpful.".to_string()));
        assert_eq!(chat.len(), 1);
        assert_eq!(chat[0].role, "user");
    }
}
