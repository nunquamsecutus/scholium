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
/// Models available for configuration. HAIKU and SONNET are used in pipelines;
/// OPUS is retained as a reference for the settings UI default options.
pub const CLAUDE_HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";
pub const CLAUDE_SONNET_MODEL: &str = "claude-sonnet-4-6";
#[allow(dead_code)]
pub const CLAUDE_OPUS_MODEL: &str = "claude-opus-4-7";

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

/// Returns true when an Ollama error body indicates the model has no chat
/// template and we should retry with /api/generate.
fn is_no_chat_template_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("does not support chat")
        || lower.contains("chat template")
        || lower.contains("no chat template")
}

async fn ollama_chat(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    messages: &[LlmMessage],
) -> Result<String, String> {
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

    // Some image-generation models that have a chat template return images
    // in message.image (string) or message.images[0] (array).
    let msg_img: Option<&str> = json["message"]["image"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            json["message"]["images"]
                .get(0)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        });
    if let Some(b64) = msg_img {
        eprintln!("[ollama chat] image data found for model {model}");
        return Ok(format!("data:image/png;base64,{b64}"));
    }

    if let Some(content) = json["message"]["content"].as_str() {
        return Ok(content.to_string());
    }

    // Unexpected shape — log the structure to aid debugging.
    eprintln!(
        "[ollama chat] unexpected response shape for model {model}.\n  keys: {keys}\n  raw (first 300): {raw:.300}",
        keys = json.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        raw = json.to_string(),
    );
    Err(format!(
        "unexpected response shape from Ollama chat (model: {model}); \
        check terminal output for details"
    ))
}

/// Fall-back for models with no chat template: POST to /api/generate, turning
/// the structured messages into a plain prompt string. The system message (if
/// any) is passed in the dedicated `system` field; the rest are concatenated as
/// `Role: content` pairs separated by blank lines.
///
/// Ollama streams responses as newline-delimited JSON (NDJSON) even when
/// `"stream":false` is requested — image-generation models in particular may
/// always stream, emitting status lines until the final `"done":true` object
/// that carries the `"images"` array.  We read the whole body as text and
/// scan for the last done-object so intermediate status lines don't mask the
/// actual result.
async fn ollama_generate(
    client: &reqwest::Client,
    url: &str,
    model: &str,
    messages: &[LlmMessage],
) -> Result<String, String> {
    let endpoint = format!("{}/api/generate", url.trim_end_matches('/'));
    let (system, chat) = split_system(messages);

    let prompt = if chat.len() == 1 {
        chat[0].content.clone()
    } else {
        chat.iter()
            .map(|m| {
                let role = if m.role == "assistant" { "Assistant" } else { "User" };
                format!("{role}: {}", m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
    });
    if let Some(sys) = system {
        body["system"] = serde_json::Value::String(sys);
    }

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

    // Read the full body as text so we can handle NDJSON streaming responses.
    let raw = res.text().await.map_err(|e| format!("read error: {e}"))?;
    eprintln!("[ollama generate] raw body ({} bytes): {:.400}", raw.len(), raw);

    // Parse NDJSON: collect every non-empty line that deserialises as JSON.
    // Prefer the last line with "done":true; fall back to the last parseable line.
    let parsed: Vec<serde_json::Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    eprintln!("[ollama generate] parsed {} NDJSON line(s) for model {model}", parsed.len());

    let json = parsed
        .iter()
        .rev()
        .find(|j| j["done"].as_bool() == Some(true))
        .or_else(|| parsed.last())
        .ok_or_else(|| format!("empty or unparseable response from Ollama generate (model: {model})"))?;

    // Diffusion / image-generation models (e.g. Flux) place their output in
    // json["image"] (string) or json["images"][0] (array) as a raw base64 PNG,
    // leaving json["response"] empty.  Check both shapes.
    // Return it as a data URL so extract_image_response can detect the format.
    let img_b64: Option<&str> = json["image"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| json["images"].get(0).and_then(|v| v.as_str()).filter(|s| !s.is_empty()));
    if let Some(b64) = img_b64 {
        eprintln!("[ollama generate] image data found for model {model} ({} base64 chars)", b64.len());
        return Ok(format!("data:image/png;base64,{b64}"));
    }

    if let Some(text) = json["response"].as_str() {
        eprintln!(
            "[ollama generate] text response for model {model} ({} chars): {:.200}",
            text.len(),
            text,
        );
        return Ok(text.to_string());
    }

    // Unexpected shape — log the full structure for diagnostics.
    eprintln!(
        "[ollama generate] unexpected final object for model {model}.\n  keys: {keys}\n  object: {obj:.300}",
        keys = json.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        obj = json.to_string(),
    );
    Err(format!(
        "unexpected response shape from Ollama generate (model: {model}); \
        check terminal output for details"
    ))
}

/// Call Ollama with /api/chat. If the model has no chat template, automatically
/// retries with /api/generate so both instruct and raw/base models work.
pub async fn call_ollama(
    url: &str,
    model: &str,
    messages: Vec<LlmMessage>,
) -> Result<String, String> {
    let client = reqwest::Client::new();
    match ollama_chat(&client, url, model, &messages).await {
        Ok(r) => Ok(r),
        Err(e) if is_no_chat_template_error(&e) => {
            ollama_generate(&client, url, model, &messages).await
        }
        Err(e) => Err(e),
    }
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
        assert_eq!(
            format!("{}/api/chat", url.trim_end_matches('/')),
            "http://localhost:11434/api/chat"
        );
        assert_eq!(
            format!("{}/api/generate", url.trim_end_matches('/')),
            "http://localhost:11434/api/generate"
        );
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

    #[test]
    fn no_chat_template_error_detected() {
        assert!(is_no_chat_template_error("model does not support chat"));
        assert!(is_no_chat_template_error(
            r#"{"error":"model \"llama2\" does not support chat, no chat template defined"}"#
        ));
        assert!(is_no_chat_template_error("500: no chat template"));
        assert!(!is_no_chat_template_error("connection refused"));
        assert!(!is_no_chat_template_error("model not found"));
    }

    #[test]
    fn generate_prompt_single_message() {
        let messages = vec![
            LlmMessage { role: "system".to_string(), content: "Be helpful.".to_string() },
            LlmMessage { role: "user".to_string(), content: "What is rust?".to_string() },
        ];
        let (system, chat) = split_system(&messages);
        assert_eq!(system.as_deref(), Some("Be helpful."));
        // single non-system message → raw content, no "User:" prefix
        let prompt = if chat.len() == 1 {
            chat[0].content.clone()
        } else {
            chat.iter()
                .map(|m| {
                    let role = if m.role == "assistant" { "Assistant" } else { "User" };
                    format!("{role}: {}", m.content)
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        assert_eq!(prompt, "What is rust?");
    }

    #[test]
    fn ollama_generate_image_string_becomes_data_url() {
        // Flux (and similar) returns a top-level "image" string, not an array.
        let json: serde_json::Value = serde_json::json!({
            "model": "x/flux2-klein:4b",
            "response": "",
            "image": "iVBORfakebase64==",
            "done": true
        });
        let img_b64: Option<&str> = json["image"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| json["images"].get(0).and_then(|v| v.as_str()).filter(|s| !s.is_empty()));
        let result = img_b64
            .map(|b64| format!("data:image/png;base64,{b64}"))
            .unwrap_or_else(|| json["response"].as_str().unwrap_or("").to_string());
        assert_eq!(result, "data:image/png;base64,iVBORfakebase64==");
    }

    #[test]
    fn ollama_generate_images_array_becomes_data_url() {
        // Some models use an "images" array instead of a singular "image" field.
        let json: serde_json::Value = serde_json::json!({
            "model": "somemodel:latest",
            "response": "",
            "images": ["iVBORfakebase64=="],
            "done": true
        });
        let img_b64: Option<&str> = json["image"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| json["images"].get(0).and_then(|v| v.as_str()).filter(|s| !s.is_empty()));
        let result = img_b64
            .map(|b64| format!("data:image/png;base64,{b64}"))
            .unwrap_or_else(|| json["response"].as_str().unwrap_or("").to_string());
        assert_eq!(result, "data:image/png;base64,iVBORfakebase64==");
    }

    #[test]
    fn ollama_generate_text_model_uses_response_field() {
        let json: serde_json::Value = serde_json::json!({
            "model": "llama3.2",
            "response": "Here is the SVG: <svg/>",
            "done": true
        });
        // No "images" key → should fall through to "response"
        let has_images = json["images"].get(0).and_then(|v| v.as_str()).filter(|s| !s.is_empty()).is_some();
        assert!(!has_images);
        assert_eq!(json["response"].as_str().unwrap(), "Here is the SVG: <svg/>");
    }

    #[test]
    fn generate_prompt_multi_turn() {
        let messages = vec![
            LlmMessage { role: "user".to_string(), content: "Hi".to_string() },
            LlmMessage { role: "assistant".to_string(), content: "Hello!".to_string() },
            LlmMessage { role: "user".to_string(), content: "What is 2+2?".to_string() },
        ];
        let (_, chat) = split_system(&messages);
        let prompt = chat.iter()
            .map(|m| {
                let role = if m.role == "assistant" { "Assistant" } else { "User" };
                format!("{role}: {}", m.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(prompt, "User: Hi\n\nAssistant: Hello!\n\nUser: What is 2+2?");
    }
}
