mod chapter;
mod define;
mod edupage;
mod expand;
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
    reading_level: String,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_initial_messages(&topic, &reading_level);
    dispatch_llm(&settings, messages).await
}

#[tauri::command]
async fn continue_onboarding(
    state: tauri::State<'_, AppState>,
    topic: String,
    reading_level: String,
    conversation: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_continuation_messages(&topic, &reading_level, conversation);
    dispatch_llm(&settings, messages).await
}

#[tauri::command]
async fn generate_lesson_plan(
    state: tauri::State<'_, AppState>,
    topic: String,
    reading_level: String,
    conversation: Vec<llm::LlmMessage>,
) -> Result<onboarding::GeneratedPlan, String> {
    let settings = state.settings.lock().unwrap().clone();
    let messages = onboarding::build_plan_messages(&topic, &reading_level, conversation);
    let response = dispatch_llm(&settings, messages).await?;
    onboarding::parse_plan(&response)
}

// ── Chapter generation ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateChapterResult {
    pub content: String,
    pub manifest: manifest::Manifest,
}

#[tauri::command]
async fn generate_chapter(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
) -> Result<GenerateChapterResult, String> {
    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;

    let mut book = manifest::load(&book_path)?;

    let idx = book
        .lesson_plan
        .chapters
        .iter()
        .position(|ch| ch.id == chapter_id)
        .ok_or_else(|| format!("chapter {chapter_id} not found"))?;

    // Mark as in-progress before the LLM call so a crash leaves a visible signal.
    book.lesson_plan.chapters[idx].status = manifest::ChapterStatus::Generating;
    manifest::save(&book, &book_path)?;

    let messages = chapter::build_messages(&book, &chapter_id)?;
    let settings = state.settings.lock().unwrap().clone();
    let content = match dispatch_llm(&settings, messages).await {
        Ok(c) => c,
        Err(e) => {
            // Roll back the status so the chapter stays actionable.
            book.lesson_plan.chapters[idx].status = manifest::ChapterStatus::Planned;
            manifest::save(&book, &book_path).ok();
            return Err(e);
        }
    };

    let chapter_file = book.lesson_plan.chapters[idx].file.clone();
    let chapter_path = book_path
        .parent()
        .ok_or("invalid book path")?
        .join(&chapter_file);

    if let Some(dir) = chapter_path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("failed to create chapter directory: {e}"))?;
    }
    let page = edupage::create(
        &chapter_id,
        &book.lesson_plan.chapters[idx].title,
        book.lesson_plan.chapters[idx].description.as_deref(),
        &content,
    );
    std::fs::write(&chapter_path, &page)
        .map_err(|e| format!("failed to write chapter file: {e}"))?;

    book.lesson_plan.chapters[idx].status = manifest::ChapterStatus::Generated;
    book.metadata.modified = chrono::Utc::now().to_rfc3339();
    manifest::save(&book, &book_path)?;

    Ok(GenerateChapterResult { content, manifest: book })
}

#[tauri::command]
fn read_chapter(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
) -> Result<ChapterContent, String> {
    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let page = edupage::read(&raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

// ── Definition (LLM, context-aware) ──────────────────────────────────────────

#[tauri::command]
async fn define_word(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    word: String,
    occurrence_index: u32,
    context: String,
) -> Result<ChapterContent, String> {
    if word.trim().is_empty() {
        return Err("empty word".to_string());
    }
    if context.trim().is_empty() {
        return Err("empty context".to_string());
    }

    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;
    let book = manifest::load(&book_path)?;
    let reading_level = book.metadata.reading_level.as_deref().unwrap_or("adult");
    let topic = Some(book.metadata.topic.as_str()).filter(|s| !s.is_empty());

    let settings = state.settings.lock().unwrap().clone();
    let messages = define::build_messages(&word, &context, reading_level, topic);
    let raw = dispatch_llm(&settings, messages).await?;
    let definition = define::clean_response(&word, &raw);
    if definition.is_empty() {
        return Err("model returned an empty definition".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_note(
        &raw_file,
        edupage::NoteType::Definition,
        &word,
        occurrence_index,
        &definition,
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

// ── Notes (marginalia / footnotes / endnotes) ─────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContent {
    pub content: String,
    pub notes: Vec<edupage::NoteWithBody>,
}

fn chapter_path_for(
    state: &tauri::State<'_, AppState>,
    chapter_id: &str,
) -> Result<std::path::PathBuf, String> {
    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;
    let book = manifest::load(&book_path)?;
    let chapter = book
        .lesson_plan
        .chapters
        .iter()
        .find(|ch| ch.id == chapter_id)
        .ok_or_else(|| format!("chapter {chapter_id} not found"))?;
    Ok(book_path
        .parent()
        .ok_or("invalid book path")?
        .join(&chapter.file))
}

#[tauri::command]
async fn add_footnote(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    selection: String,
    occurrence_index: u32,
    context: String,
) -> Result<ChapterContent, String> {
    if selection.trim().is_empty() {
        return Err("empty selection".to_string());
    }
    if context.trim().is_empty() {
        return Err("empty context".to_string());
    }

    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;
    let book = manifest::load(&book_path)?;
    let reading_level = book.metadata.reading_level.as_deref().unwrap_or("adult");
    let topic = Some(book.metadata.topic.as_str()).filter(|s| !s.is_empty());

    let settings = state.settings.lock().unwrap().clone();
    let messages = expand::build_footnote_messages(&selection, &context, reading_level, topic);
    let raw = dispatch_llm(&settings, messages).await?;
    let body = expand::clean_footnote_response(&raw);
    if body.is_empty() {
        return Err("model returned an empty footnote".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_note(
        &raw_file,
        edupage::NoteType::Footnote,
        &selection,
        occurrence_index,
        &body,
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

#[tauri::command]
async fn add_endnote(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    selection: String,
    occurrence_index: u32,
    context: String,
) -> Result<ChapterContent, String> {
    if selection.trim().is_empty() {
        return Err("empty selection".to_string());
    }
    if context.trim().is_empty() {
        return Err("empty context".to_string());
    }

    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;
    let book = manifest::load(&book_path)?;
    let reading_level = book.metadata.reading_level.as_deref().unwrap_or("adult");
    let topic = Some(book.metadata.topic.as_str()).filter(|s| !s.is_empty());

    let settings = state.settings.lock().unwrap().clone();
    let messages = expand::build_endnote_messages(&selection, &context, reading_level, topic);
    let raw = dispatch_llm(&settings, messages).await?;
    let body = expand::clean_endnote_response(&raw);
    if body.is_empty() {
        return Err("model returned an empty endnote".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_note(
        &raw_file,
        edupage::NoteType::Endnote,
        &selection,
        occurrence_index,
        &body,
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendixResult {
    pub content: String,
    pub notes: Vec<edupage::NoteWithBody>,
    pub manifest: manifest::Manifest,
}

fn appendix_title_from(seq: u32, selection: &str) -> String {
    let trimmed = selection.trim();
    let snippet: String = if trimmed.chars().count() > 40 {
        let mut s: String = trimmed.chars().take(40).collect();
        s.push('…');
        s
    } else {
        trimmed.to_string()
    };
    format!("Appendix {}: {}", seq, snippet)
}

#[tauri::command]
async fn add_appendix(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    selection: String,
    occurrence_index: u32,
    context: String,
) -> Result<AppendixResult, String> {
    if selection.trim().is_empty() {
        return Err("empty selection".to_string());
    }
    if context.trim().is_empty() {
        return Err("empty context".to_string());
    }

    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("no book is open")?;
    let mut book = manifest::load(&book_path)?;
    let reading_level = book
        .metadata
        .reading_level
        .as_deref()
        .unwrap_or("adult")
        .to_string();
    let topic = book.metadata.topic.clone();

    // Sequence number is one past the count of existing chapters whose title
    // starts with "Appendix" — deletions are not supported yet, so this
    // produces a stable monotonically increasing number per book.
    let seq = book
        .lesson_plan
        .chapters
        .iter()
        .filter(|c| c.title.starts_with("Appendix"))
        .count() as u32
        + 1;

    let settings = state.settings.lock().unwrap().clone();
    let topic_ref = Some(topic.as_str()).filter(|s| !s.is_empty());
    let messages =
        expand::build_appendix_messages(&selection, &context, &reading_level, topic_ref);
    let raw_llm = dispatch_llm(&settings, messages).await?;
    let content = expand::clean_appendix_response(&raw_llm);
    if content.is_empty() {
        return Err("model returned an empty appendix".to_string());
    }

    let appendix_id = format!("ap-{}", seq);
    let appendix_title = appendix_title_from(seq, &selection);
    let appendix_file = format!("chapters/{}.edupage", appendix_id);

    let book_dir = book_path.parent().ok_or("invalid book path")?.to_path_buf();
    let appendix_path = book_dir.join(&appendix_file);
    if let Some(parent) = appendix_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create chapters dir: {e}"))?;
    }
    let edupage_raw = edupage::create(&appendix_id, &appendix_title, None, &content);
    std::fs::write(&appendix_path, &edupage_raw)
        .map_err(|e| format!("failed to write appendix file: {e}"))?;

    book.lesson_plan.chapters.push(manifest::Chapter {
        id: appendix_id.clone(),
        title: appendix_title.clone(),
        description: None,
        file: appendix_file.clone(),
        status: manifest::ChapterStatus::Generated,
    });
    book.metadata.modified = chrono::Utc::now().to_rfc3339();
    manifest::save(&book, &book_path)?;

    // Now revise the source chapter to include the cross-reference.
    let source_chapter = book
        .lesson_plan
        .chapters
        .iter()
        .find(|c| c.id == chapter_id)
        .ok_or_else(|| format!("chapter {chapter_id} not found"))?;
    let source_path = book_dir.join(&source_chapter.file);
    let source_raw = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("failed to read source chapter: {e}"))?;
    let new_source_raw =
        edupage::insert_appendix_ref(&source_raw, seq, &selection, occurrence_index)?;
    std::fs::write(&source_path, &new_source_raw)
        .map_err(|e| format!("failed to write source chapter: {e}"))?;

    let page = edupage::read(&new_source_raw)?;
    Ok(AppendixResult {
        content: page.content,
        notes: page.notes,
        manifest: book,
    })
}

#[tauri::command]
fn delete_note(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    note_id: u32,
) -> Result<ChapterContent, String> {
    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let new_raw = edupage::delete_note(&raw, note_id)?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;
    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

#[tauri::command]
fn add_note(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    note_type: String,
    word: String,
    occurrence_index: u32,
    body: String,
) -> Result<ChapterContent, String> {
    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;

    let nt = match note_type.as_str() {
        "definition" => edupage::NoteType::Definition,
        other => return Err(format!("unknown note type: {other}")),
    };

    let (new_raw, _) = edupage::add_note(&raw, nt, &word, occurrence_index, &body)?;

    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent {
        content: page.content,
        notes: page.notes,
    })
}

// ── Book management ───────────────────────────────────────────────────────────

#[tauri::command]
fn create_book(
    state: tauri::State<'_, AppState>,
    topic: String,
    dialog_path: String,
    plan: onboarding::GeneratedPlan,
    reading_level: String,
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
            reading_level: Some(reading_level),
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
            generate_chapter,
            read_chapter,
            define_word,
            add_footnote,
            add_endnote,
            add_appendix,
            add_note,
            delete_note,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
