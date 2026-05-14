use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

pub async fn call_claude(api_key: &str, messages: Vec<LlmMessage>) -> Result<String, String> {
    let client = reqwest::Client::new();

    // Claude uses a top-level `system` field rather than a system role in messages.
    let system = messages.iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());
    let chat_messages: Vec<&LlmMessage> = messages.iter()
        .filter(|m| m.role != "system")
        .collect();

    let mut body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 8192,
        "messages": chat_messages,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

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
