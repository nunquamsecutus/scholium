use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Claude,
    Ollama,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub provider: LlmProvider,
    /// Never serialized — stays server-side only.
    pub claude_api_key: Option<String>,
    pub ollama_url: String,
    pub ollama_model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Ollama,
            claude_api_key: None,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "llama3.2".to_string(),
        }
    }
}

/// Safe subset exposed to the frontend — no secrets.
#[derive(Debug, Clone, Serialize)]
pub struct PublicSettings {
    pub provider: LlmProvider,
    pub claude_configured: bool,
    pub ollama_url: String,
    pub ollama_model: String,
}

impl From<&Settings> for PublicSettings {
    fn from(s: &Settings) -> Self {
        Self {
            provider: s.provider.clone(),
            claude_configured: s.claude_api_key.is_some(),
            ollama_url: s.ollama_url.clone(),
            ollama_model: s.ollama_model.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ollama() {
        let s = Settings::default();
        assert_eq!(s.provider, LlmProvider::Ollama);
        assert_eq!(s.ollama_url, "http://localhost:11434");
        assert_eq!(s.ollama_model, "llama3.2");
        assert!(s.claude_api_key.is_none());
    }

    #[test]
    fn public_settings_hides_key_but_signals_configured() {
        let s = Settings {
            provider: LlmProvider::Claude,
            claude_api_key: Some("sk-secret".to_string()),
            ..Settings::default()
        };
        let public = PublicSettings::from(&s);
        assert_eq!(public.provider, LlmProvider::Claude);
        assert!(public.claude_configured);
    }

    #[test]
    fn public_settings_shows_unconfigured_when_no_key() {
        let public = PublicSettings::from(&Settings::default());
        assert!(!public.claude_configured);
    }
}
