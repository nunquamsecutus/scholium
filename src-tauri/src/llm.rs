use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub content: String,
}

pub async fn call_claude(api_key: &str, messages: Vec<LlmMessage>) -> Result<String, String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 8192,
        "messages": messages,
    });

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
        let msg = LlmMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"], "Hello");
    }

    #[test]
    fn ollama_url_trailing_slash_is_trimmed() {
        // Verify the endpoint construction doesn't double-slash.
        let url = "http://localhost:11434/";
        let endpoint = format!("{}/api/chat", url.trim_end_matches('/'));
        assert_eq!(endpoint, "http://localhost:11434/api/chat");
    }
}
