mod llm;
mod settings;

use settings::{LlmProvider, PublicSettings, Settings};
use tauri::Manager;
use tauri_plugin_cli::CliExt;

pub struct AppState {
    pub settings: std::sync::Mutex<Settings>,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> PublicSettings {
    PublicSettings::from(&*state.settings.lock().unwrap())
}

#[tauri::command]
async fn call_llm(
    state: tauri::State<'_, AppState>,
    messages: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    match settings.provider {
        LlmProvider::Claude => {
            let key = settings
                .claude_api_key
                .ok_or("Claude API key not configured — restart with --api-key or set ANTHROPIC_API_KEY")?;
            llm::call_claude(&key, messages).await
        }
        LlmProvider::Ollama => {
            llm::call_ollama(&settings.ollama_url, &settings.ollama_model, messages).await
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_cli::init())
        .setup(|app| {
            let mut settings = Settings::default();

            // CLI args take first priority.
            if let Ok(matches) = app.cli().matches() {
                if let Some(arg) = matches.args.get("api-key") {
                    if let serde_json::Value::String(key) = &arg.value {
                        if !key.is_empty() {
                            settings.claude_api_key = Some(key.clone());
                            settings.provider = LlmProvider::Claude;
                        }
                    }
                }
                if let Some(arg) = matches.args.get("provider") {
                    if let serde_json::Value::String(p) = &arg.value {
                        match p.as_str() {
                            "claude" => settings.provider = LlmProvider::Claude,
                            "ollama" => settings.provider = LlmProvider::Ollama,
                            other => eprintln!("unknown provider '{other}', ignoring"),
                        }
                    }
                }
                if let Some(arg) = matches.args.get("ollama-url") {
                    if let serde_json::Value::String(url) = &arg.value {
                        settings.ollama_url = url.clone();
                    }
                }
                if let Some(arg) = matches.args.get("ollama-model") {
                    if let serde_json::Value::String(model) = &arg.value {
                        settings.ollama_model = model.clone();
                    }
                }
            }

            // ANTHROPIC_API_KEY env var as fallback when no CLI key was given.
            if settings.claude_api_key.is_none() {
                if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                    if !key.is_empty() {
                        settings.claude_api_key = Some(key);
                        settings.provider = LlmProvider::Claude;
                    }
                }
            }

            app.manage(AppState {
                settings: std::sync::Mutex::new(settings),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_settings, call_llm])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
