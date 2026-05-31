use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    Claude,
    Ollama,
}

impl Default for LlmProvider {
    fn default() -> Self {
        LlmProvider::Ollama
    }
}

/// Desired image quality. Persisted but not yet wired to generation behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageQuality {
    Fast,
    Medium,
    High,
}

impl Default for ImageQuality {
    fn default() -> Self {
        ImageQuality::Medium
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// Provider used for general text tasks (onboarding, chapters, notes, …).
    pub provider: LlmProvider,
    /// Loaded from the OS keyring; never written to the config file or sent
    /// to the frontend.
    pub claude_api_key: Option<String>,
    pub ollama_url: String,
    pub ollama_model: String,
    /// Provider used for image/diagram generation steps (composition, render,
    /// polish). May differ from `provider` so a fast local model handles
    /// generation while a smarter remote model handles text tasks.
    pub image_provider: LlmProvider,
    /// Claude model used when `image_provider` is Claude.
    pub claude_image_model: String,
    /// Ollama model used when `image_provider` is Ollama.
    pub ollama_image_model: String,
    pub image_quality: ImageQuality,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Ollama,
            claude_api_key: None,
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "llama3.2".to_string(),
            image_provider: LlmProvider::Ollama,
            claude_image_model: "claude-sonnet-4-6".to_string(),
            ollama_image_model: "llama3.2".to_string(),
            image_quality: ImageQuality::Medium,
        }
    }
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_ollama_model() -> String {
    "llama3.2".to_string()
}

fn default_claude_image_model() -> String {
    "claude-sonnet-4-6".to_string()
}

fn default_ollama_image_model() -> String {
    "llama3.2".to_string()
}

/// The non-secret subset persisted to the on-disk config file. Field-level
/// defaults let older or partial files load without error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredConfig {
    #[serde(default)]
    pub provider: LlmProvider,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_ollama_model")]
    pub ollama_model: String,
    #[serde(default)]
    pub image_provider: LlmProvider,
    #[serde(default = "default_claude_image_model")]
    pub claude_image_model: String,
    #[serde(default = "default_ollama_image_model")]
    pub ollama_image_model: String,
    #[serde(default)]
    pub image_quality: ImageQuality,
}

impl Settings {
    /// Overlay persisted (non-secret) config onto these settings.
    pub fn apply_config(&mut self, c: &StoredConfig) {
        self.provider = c.provider.clone();
        self.ollama_url = c.ollama_url.clone();
        self.ollama_model = c.ollama_model.clone();
        self.image_provider = c.image_provider.clone();
        self.claude_image_model = c.claude_image_model.clone();
        self.ollama_image_model = c.ollama_image_model.clone();
        self.image_quality = c.image_quality.clone();
    }

    pub fn to_config(&self) -> StoredConfig {
        StoredConfig {
            provider: self.provider.clone(),
            ollama_url: self.ollama_url.clone(),
            ollama_model: self.ollama_model.clone(),
            image_provider: self.image_provider.clone(),
            claude_image_model: self.claude_image_model.clone(),
            ollama_image_model: self.ollama_image_model.clone(),
            image_quality: self.image_quality.clone(),
        }
    }
}

/// Safe subset exposed to the frontend — no secrets.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    pub provider: LlmProvider,
    pub claude_configured: bool,
    pub ollama_url: String,
    pub ollama_model: String,
    pub image_provider: LlmProvider,
    pub claude_image_model: String,
    pub ollama_image_model: String,
    pub image_quality: ImageQuality,
}

impl From<&Settings> for PublicSettings {
    fn from(s: &Settings) -> Self {
        Self {
            provider: s.provider.clone(),
            claude_configured: s.claude_api_key.is_some(),
            ollama_url: s.ollama_url.clone(),
            ollama_model: s.ollama_model.clone(),
            image_provider: s.image_provider.clone(),
            claude_image_model: s.claude_image_model.clone(),
            ollama_image_model: s.ollama_image_model.clone(),
            image_quality: s.image_quality.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_ollama_medium() {
        let s = Settings::default();
        assert_eq!(s.provider, LlmProvider::Ollama);
        assert_eq!(s.ollama_url, "http://localhost:11434");
        assert_eq!(s.ollama_model, "llama3.2");
        assert_eq!(s.image_provider, LlmProvider::Ollama);
        assert_eq!(s.claude_image_model, "claude-sonnet-4-6");
        assert_eq!(s.ollama_image_model, "llama3.2");
        assert_eq!(s.image_quality, ImageQuality::Medium);
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

    #[test]
    fn config_round_trips_through_settings() {
        let mut s = Settings::default();
        s.provider = LlmProvider::Claude;
        s.ollama_url = "http://example:1234".to_string();
        s.image_provider = LlmProvider::Claude;
        s.claude_image_model = "claude-opus-4-7".to_string();
        s.ollama_image_model = "mistral".to_string();
        s.image_quality = ImageQuality::High;
        let cfg = s.to_config();
        let mut restored = Settings::default();
        restored.apply_config(&cfg);
        assert_eq!(restored.provider, LlmProvider::Claude);
        assert_eq!(restored.ollama_url, "http://example:1234");
        assert_eq!(restored.image_provider, LlmProvider::Claude);
        assert_eq!(restored.claude_image_model, "claude-opus-4-7");
        assert_eq!(restored.ollama_image_model, "mistral");
        assert_eq!(restored.image_quality, ImageQuality::High);
    }

    #[test]
    fn partial_config_uses_field_defaults() {
        let cfg: StoredConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.provider, LlmProvider::Ollama);
        assert_eq!(cfg.ollama_url, "http://localhost:11434");
        assert_eq!(cfg.image_quality, ImageQuality::Medium);
    }

    #[test]
    fn config_serializes_camel_case_without_secrets() {
        let json = serde_json::to_string(&Settings::default().to_config()).unwrap();
        assert!(json.contains("ollamaUrl"));
        assert!(json.contains("imageQuality"));
        assert!(json.contains("imageProvider"));
        assert!(json.contains("claudeImageModel"));
        assert!(json.contains("ollamaImageModel"));
        assert!(!json.contains("claude_api_key"));
        assert!(!json.contains("apiKey"));
    }

    #[test]
    fn partial_config_uses_image_model_defaults() {
        let cfg: StoredConfig = serde_json::from_str(r#"{"provider":"claude"}"#).unwrap();
        assert_eq!(cfg.image_provider, LlmProvider::Ollama);
        assert_eq!(cfg.claude_image_model, "claude-sonnet-4-6");
        assert_eq!(cfg.ollama_image_model, "llama3.2");
    }
}
