mod llm;
mod manifest;
mod onboarding;
mod settings;

use settings::{LlmProvider, PublicSettings, Settings};
use tauri::Manager;
use tauri_plugin_cli::CliExt;

pub struct AppState {
    pub settings: std::sync::Mutex<Settings>,
    pub book_path: std::sync::Mutex<Option<std::path::PathBuf>>,
}

// ── Shared LLM dispatch ──────────────────────────────────────────────────────

async fn dispatch_llm(
    settings: &Settings,
    messages: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    match settings.provider {
        LlmProvider::Claude => {
            let key = settings.claude_api_key.as_ref().ok_or(
                "Claude API key not configured — restart with --api-key or set ANTHROPIC_API_KEY",
            )?;
            llm::call_claude(key, messages).await
        }
        LlmProvider::Ollama => {
            llm::call_ollama(&settings.ollama_url, &settings.ollama_model, messages).await
        }
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> PublicSettings {
    PublicSettings::from(&*state.settings.lock().unwrap())
}

// ── Generic LLM call (used by future chapter generation) ─────────────────────

#[tauri::command]
async fn call_llm(
    state: tauri::State<'_, AppState>,
    messages: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    dispatch_llm(&settings, messages).await
}

// ── Onboarding ───────────────────────────────────────────────────────────────

#[tauri::command]
async fn begin_onboarding(
    state: tauri::State<'_, AppState>,
    topic: String,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_initial_messages(&topic);
    dispatch_llm(&settings, messages).await
}

#[tauri::command]
async fn continue_onboarding(
    state: tauri::State<'_, AppState>,
    topic: String,
    conversation: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_continuation_messages(&topic, conversation);
    dispatch_llm(&settings, messages).await
}

#[tauri::command]
async fn generate_lesson_plan(
    state: tauri::State<'_, AppState>,
    topic: String,
    conversation: Vec<llm::LlmMessage>,
) -> Result<onboarding::GeneratedPlan, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_plan_messages(&topic, conversation);
    let response = dispatch_llm(&settings, messages).await?;
    onboarding::parse_plan(&response)
}

// ── Book management ───────────────────────────────────────────────────────────

#[tauri::command]
fn create_book(
    state: tauri::State<'_, AppState>,
    topic: String,
    dialog_path: String,
    plan: onboarding::GeneratedPlan,
) -> Result<manifest::Manifest, String> {
    let dialog_path = std::path::PathBuf::from(&dialog_path);

    let stem = dialog_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid save path")?
        .to_string();
    let parent = dialog_path.parent().ok_or("invalid save path")?;

    let book_dir = parent.join(&stem);
    let manifest_path = book_dir.join(format!("{stem}.edubook"));

    std::fs::create_dir_all(book_dir.join("chapters"))
        .map_err(|e| format!("failed to create chapters dir: {e}"))?;
    std::fs::create_dir_all(book_dir.join("images"))
        .map_err(|e| format!("failed to create images dir: {e}"))?;

    let now = chrono::Utc::now().to_rfc3339();
    let title = title_case(&topic);

    let book = manifest::Manifest {
        version: 1,
        metadata: manifest::Metadata {
            title,
            subtitle: None,
            prompt: topic.clone(),
            topic,
            created: now.clone(),
            modified: now,
            description: Some(plan.description),
            reading_level: Some(plan.reading_level),
            prior_knowledge: Some(plan.prior_knowledge),
        },
        lesson_plan: manifest::LessonPlan {
            summary: plan.summary,
            chapters: plan.lesson_plan.chapters,
        },
    };

    manifest::save(&book, &manifest_path)?;
    *state.book_path.lock().unwrap() = Some(manifest_path);

    Ok(book)
}

#[tauri::command]
fn load_book(
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<manifest::Manifest, String> {
    let path = std::path::PathBuf::from(&path);
    let book = manifest::load(&path)?;
    *state.book_path.lock().unwrap() = Some(path);
    Ok(book)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// ── App entry point ───────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_cli::init())
        .setup(|app| {
            let mut settings = Settings::default();

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
                book_path: std::sync::Mutex::new(None),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            call_llm,
            begin_onboarding,
            continue_onboarding,
            generate_lesson_plan,
            create_book,
            load_book,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
