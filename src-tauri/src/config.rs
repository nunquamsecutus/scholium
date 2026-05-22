//! Persistence for settings: a JSON config file for non-secret fields, and
//! the OS keyring for the Claude API key.

use crate::settings::StoredConfig;
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "com.brent.edu-harness";
const KEYRING_USER: &str = "claude_api_key";

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load_config(dir: &Path) -> Option<StoredConfig> {
    let text = std::fs::read_to_string(config_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_config(dir: &Path, config: &StoredConfig) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::write(config_path(dir), text).map_err(|e| format!("failed to write config: {e}"))
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| format!("keyring: {e}"))
}

pub fn keyring_get() -> Option<String> {
    keyring_entry().ok()?.get_password().ok()
}

pub fn keyring_set(key: &str) -> Result<(), String> {
    keyring_entry()?
        .set_password(key)
        .map_err(|e| format!("failed to store key in keyring: {e}"))
}

pub fn keyring_delete() -> Result<(), String> {
    match keyring_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("failed to remove key from keyring: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ImageQuality, LlmProvider, Settings};

    #[test]
    fn config_save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("edu-harness-cfg-{}", uuid::Uuid::new_v4()));
        let mut s = Settings::default();
        s.provider = LlmProvider::Claude;
        s.image_quality = ImageQuality::High;
        save_config(&dir, &s.to_config()).unwrap();

        let loaded = load_config(&dir).unwrap();
        assert_eq!(loaded.provider, LlmProvider::Claude);
        assert_eq!(loaded.image_quality, ImageQuality::High);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_missing_config_is_none() {
        let dir = std::env::temp_dir().join(format!("edu-harness-missing-{}", uuid::Uuid::new_v4()));
        assert!(load_config(&dir).is_none());
    }
}
