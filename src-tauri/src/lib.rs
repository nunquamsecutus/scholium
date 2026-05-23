mod chapter;
mod config;
mod define;
mod diagram;
mod edupage;
mod expand;
mod image;
mod import;
mod llm;
mod manifest;
mod onboarding;
mod rewrite;
mod settings;

use settings::{ImageQuality, LlmProvider, PublicSettings, Settings};
use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::{Emitter, Manager};
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
                "Claude API key not configured — set it in Settings (⌘,)",
            )?;
            llm::call_claude(key, llm::CLAUDE_DEFAULT_MODEL, messages).await
        }
        LlmProvider::Ollama => {
            llm::call_ollama(&settings.ollama_url, &settings.ollama_model, messages).await
        }
    }
}

// Text dispatch for the image pipeline. On Claude the caller picks the model
// (different phases use different models); Ollama uses its configured model.
async fn dispatch_image_text(
    settings: &Settings,
    model: &str,
    messages: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    match settings.provider {
        LlmProvider::Claude => {
            let key = settings
                .claude_api_key
                .as_ref()
                .ok_or("Claude API key not configured — set it in Settings (⌘,)")?;
            llm::call_claude(key, model, messages).await
        }
        LlmProvider::Ollama => {
            llm::call_ollama(&settings.ollama_url, &settings.ollama_model, messages).await
        }
    }
}

// Vision dispatch for the image pipeline. Only Claude is implemented; Ollama
// returns an error so the polish loop falls back to the current candidate.
async fn dispatch_image_vision(
    settings: &Settings,
    model: &str,
    messages: Vec<llm::LlmMessage>,
    image: &llm::LlmImage,
) -> Result<String, String> {
    match settings.provider {
        LlmProvider::Claude => {
            let key = settings
                .claude_api_key
                .as_ref()
                .ok_or("Claude API key not configured")?;
            llm::call_claude_vision(key, model, messages, image).await
        }
        LlmProvider::Ollama => {
            Err("vision polish is not supported for the Ollama provider".to_string())
        }
    }
}

// Progress emitted to the frontend during image work. `phase` tells the UI
// what's happening (composing, rendering an iteration, critiquing); `pass`
// counts completed render iterations toward `max` (the total in the active
// pipeline); `svg` is the latest candidate (empty before the first render).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ImageProgress {
    phase: String,
    pass: usize,
    max: usize,
    svg: String,
}

fn emit_progress(app: &tauri::AppHandle, phase: &str, pass: usize, max: usize, svg: &str) {
    let _ = app.emit(
        "image-progress",
        ImageProgress {
            phase: phase.to_string(),
            pass,
            max,
            svg: svg.to_string(),
        },
    );
}

// Shared polish loop used by both the image and diagram pipelines. Given a
// composition (already produced by an earlier text call) and per-kind prompt
// builders, run the configured render/critique/polish steps and return the
// best SVG produced. A failed first render is fatal; later failures stop the
// loop and keep the best result so far.
async fn run_polish_loop(
    app: &tauri::AppHandle,
    settings: &Settings,
    composition: String,
    pipeline: &image::ImagePipeline,
    build_initial_render: fn(&str) -> Vec<llm::LlmMessage>,
    build_polish: fn(&str, Option<&str>) -> Vec<llm::LlmMessage>,
    build_critique: fn() -> Vec<llm::LlmMessage>,
) -> Result<image::GeneratedImage, String> {
    use base64::Engine;

    let max = image::total_iterations(pipeline);
    let caption = image::derive_caption(&composition);

    let mut svg: Option<String> = None;
    let mut critique: Option<String> = None;
    let mut pass: usize = 0;

    for step in &pipeline.steps {
        if step.critique_first {
            if let Some(current) = &svg {
                emit_progress(app, "critique", pass, max, current);
                if let Ok(png) = image::rasterize_svg_to_png(current) {
                    let llm_image = llm::LlmImage {
                        media_type: "image/png".to_string(),
                        base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
                    };
                    let crit_messages = build_critique();
                    match dispatch_image_vision(
                        settings,
                        pipeline.critique_model,
                        crit_messages,
                        &llm_image,
                    )
                    .await
                    {
                        Ok(resp) => critique = Some(resp.trim().to_string()),
                        Err(e) => eprintln!("artifact pipeline: critique skipped: {e}"),
                    }
                }
            }
        }

        for _ in 0..step.iterations {
            pass += 1;
            emit_progress(app, "rendering", pass, max, svg.as_deref().unwrap_or(""));

            if let Some(current) = svg.clone() {
                let png = match image::rasterize_svg_to_png(&current) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("artifact pipeline: rasterize failed, stopping polish: {e}");
                        break;
                    }
                };
                let llm_image = llm::LlmImage {
                    media_type: "image/png".to_string(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
                };
                let messages = build_polish(&composition, critique.as_deref());
                match dispatch_image_vision(settings, pipeline.polish_model, messages, &llm_image)
                    .await
                {
                    Ok(resp) => match image::extract_svg(&resp) {
                        Ok(next) => {
                            svg = Some(next.clone());
                            emit_progress(app, "rendering", pass, max, &next);
                        }
                        Err(e) => {
                            eprintln!("artifact pipeline: polish returned no <svg>: {e}");
                        }
                    },
                    Err(e) => {
                        eprintln!("artifact pipeline: polish call failed, stopping: {e}");
                        break;
                    }
                }
            } else {
                let messages = build_initial_render(&composition);
                let resp =
                    dispatch_image_text(settings, pipeline.first_render_model, messages).await?;
                let initial = image::extract_svg(&resp)?;
                svg = Some(initial.clone());
                emit_progress(app, "rendering", pass, max, &initial);
            }
        }
    }

    let svg = svg.ok_or("no SVG was produced")?;
    Ok(image::GeneratedImage { svg, caption })
}

async fn run_image_pipeline(
    app: &tauri::AppHandle,
    settings: &Settings,
    source: &str,
    context: &str,
    extra_instruction: Option<&str>,
) -> Result<image::GeneratedImage, String> {
    let pipeline = image::pipeline_for(&settings.image_quality);
    let max = image::total_iterations(&pipeline);

    emit_progress(app, "composition", 0, max, "");
    let comp_messages = image::build_composition_messages(source, context, extra_instruction);
    let composition = dispatch_image_text(settings, pipeline.composition_model, comp_messages)
        .await?
        .trim()
        .to_string();

    run_polish_loop(
        app,
        settings,
        composition,
        &pipeline,
        image::build_initial_render_messages,
        image::build_polish_messages,
        image::build_critique_messages,
    )
    .await
}

// Sonnet plans + draws, Haiku evaluates. The loop exits early when both of
// Haiku's 5-point scores (help / ease) reach diagram::SATISFACTORY_SCORE, or
// after diagram::MAX_ITERATIONS improvement passes.
async fn run_diagram_pipeline(
    app: &tauri::AppHandle,
    settings: &Settings,
    source: &str,
    context: &str,
    reading_level: &str,
    original_prompt: Option<&str>,
) -> Result<image::GeneratedImage, String> {
    use base64::Engine;

    let max = diagram::TOTAL_PASSES;

    // 1. Composition (Sonnet text).
    emit_progress(app, "composition", 0, max, "");
    let comp_messages =
        diagram::build_composition_messages(source, context, reading_level, original_prompt);
    let composition = dispatch_image_text(settings, llm::CLAUDE_SONNET_MODEL, comp_messages)
        .await?
        .trim()
        .to_string();
    let caption = image::derive_caption(&composition);

    // 2. Initial render (Sonnet text).
    let mut pass: usize = 1;
    emit_progress(app, "rendering", pass, max, "");
    let init_messages = diagram::build_initial_render_messages(&composition);
    let resp = dispatch_image_text(settings, llm::CLAUDE_SONNET_MODEL, init_messages).await?;
    let mut svg = image::extract_svg(&resp)?;
    emit_progress(app, "rendering", pass, max, &svg);

    // 3. Evaluate-and-improve loop (Haiku judges, Sonnet redraws).
    for _ in 0..diagram::MAX_ITERATIONS {
        emit_progress(app, "critique", pass, max, &svg);

        let png = match image::rasterize_svg_to_png(&svg) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("diagram pipeline: rasterize failed, stopping: {e}");
                break;
            }
        };
        let llm_image = llm::LlmImage {
            media_type: "image/png".to_string(),
            base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
        };
        let eval_messages = diagram::build_evaluation_messages(source);
        let score = match dispatch_image_vision(
            settings,
            llm::CLAUDE_HAIKU_MODEL,
            eval_messages,
            &llm_image,
        )
        .await
        {
            Ok(resp) => match diagram::parse_diagram_score(&resp) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("diagram pipeline: score parse failed, keeping current: {e}");
                    break;
                }
            },
            Err(e) => {
                eprintln!("diagram pipeline: eval call skipped, keeping current: {e}");
                break;
            }
        };

        if score.is_satisfactory() {
            break;
        }

        pass += 1;
        emit_progress(app, "rendering", pass, max, &svg);
        let improve_messages = diagram::build_improve_messages(&composition, &svg, &score);
        match dispatch_image_text(settings, llm::CLAUDE_SONNET_MODEL, improve_messages).await {
            Ok(resp) => match image::extract_svg(&resp) {
                Ok(next) => {
                    svg = next.clone();
                    emit_progress(app, "rendering", pass, max, &svg);
                }
                Err(e) => {
                    eprintln!("diagram pipeline: improve returned no <svg>, keeping current: {e}");
                    break;
                }
            },
            Err(e) => {
                eprintln!("diagram pipeline: improve call failed, keeping current: {e}");
                break;
            }
        }
    }

    Ok(image::GeneratedImage { svg, caption })
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> PublicSettings {
    PublicSettings::from(&*state.settings.lock().unwrap())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    provider: String,
    ollama_url: String,
    ollama_model: String,
    image_quality: String,
    /// Some(non-empty) sets the key; None leaves it unchanged.
    #[serde(default)]
    claude_api_key: Option<String>,
    /// When true, removes the stored key (overrides claude_api_key).
    #[serde(default)]
    clear_claude_key: bool,
}

#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    update: SettingsUpdate,
) -> Result<PublicSettings, String> {
    let provider = match update.provider.as_str() {
        "claude" => LlmProvider::Claude,
        "ollama" => LlmProvider::Ollama,
        other => return Err(format!("unknown provider: {other}")),
    };
    let image_quality = match update.image_quality.as_str() {
        "fast" => ImageQuality::Fast,
        "medium" => ImageQuality::Medium,
        "high" => ImageQuality::High,
        other => return Err(format!("unknown image quality: {other}")),
    };

    let mut settings = state.settings.lock().unwrap();
    settings.provider = provider;
    settings.ollama_url = update.ollama_url;
    settings.ollama_model = update.ollama_model;
    settings.image_quality = image_quality;

    if update.clear_claude_key {
        config::keyring_delete()?;
        settings.claude_api_key = None;
    } else if let Some(key) = update.claude_api_key {
        let key = key.trim();
        if !key.is_empty() {
            config::keyring_set(key)?;
            settings.claude_api_key = Some(key.to_string());
        }
    }

    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    config::save_config(&dir, &settings.to_config())?;

    Ok(PublicSettings::from(&*settings))
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
    Ok(ChapterContent::from_page(page))
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
    Ok(ChapterContent::from_page(page))
}

// ── Notes (marginalia / footnotes / endnotes) ─────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterContent {
    pub content: String,
    pub notes: Vec<edupage::NoteWithBody>,
    pub artifacts: Vec<edupage::ArtifactWithBody>,
}

impl ChapterContent {
    fn from_page(page: edupage::EduPage) -> Self {
        ChapterContent {
            content: page.content,
            notes: page.notes,
            artifacts: page.artifacts,
        }
    }
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
    Ok(ChapterContent::from_page(page))
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
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
async fn add_image(
    app: tauri::AppHandle,
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

    // The pipeline doesn't consult reading_level or topic; the prompts speak
    // directly to "professional quality art" without level-specific phrasing.
    let settings = state.settings.lock().unwrap().clone();
    let generated = run_image_pipeline(&app, &settings, &selection, &context, None).await?;
    let aspect = image::aspect_ratio_of(&generated.svg);

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_artifact(
        &raw_file,
        &selection,
        occurrence_index,
        &generated.svg,
        &generated.caption,
        aspect,
        "image",
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
async fn add_diagram(
    app: tauri::AppHandle,
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
    // The original learning prompt becomes the domain hint, skipped when empty
    // (which is the case for imported books).
    let original_prompt = Some(book.metadata.prompt.as_str()).filter(|s| !s.is_empty());

    let settings = state.settings.lock().unwrap().clone();
    let generated = run_diagram_pipeline(
        &app,
        &settings,
        &selection,
        &context,
        reading_level,
        original_prompt,
    )
    .await?;
    let aspect = image::aspect_ratio_of(&generated.svg);

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_artifact(
        &raw_file,
        &selection,
        occurrence_index,
        &generated.svg,
        &generated.caption,
        aspect,
        "diagram",
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
async fn regenerate_artifact(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    artifact_id: u32,
    instruction: String,
    context: String,
) -> Result<ChapterContent, String> {
    if instruction.trim().is_empty() {
        return Err("empty instruction".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let page = edupage::read(&raw_file)?;
    let artifact = page
        .artifacts
        .iter()
        .find(|a| a.id == artifact_id)
        .ok_or_else(|| format!("artifact {artifact_id} not found"))?;
    let source = artifact.source.clone();
    let semantic_type = artifact.semantic_type.clone();

    let settings = state.settings.lock().unwrap().clone();
    // Dispatch to the pipeline matching the artifact's kind. The user's
    // instruction is folded into the composition step.
    let generated = if semantic_type == "diagram" {
        let book_path = state
            .book_path
            .lock()
            .unwrap()
            .clone()
            .ok_or("no book is open")?;
        let book = manifest::load(&book_path)?;
        let reading_level = book.metadata.reading_level.as_deref().unwrap_or("adult");
        let original_prompt = Some(book.metadata.prompt.as_str()).filter(|s| !s.is_empty());
        let combined_source = format!("{source} (with this change: {instruction})");
        run_diagram_pipeline(
            &app,
            &settings,
            &combined_source,
            &context,
            reading_level,
            original_prompt,
        )
        .await?
    } else {
        run_image_pipeline(&app, &settings, &source, &context, Some(&instruction)).await?
    };
    let new_aspect = image::aspect_ratio_of(&generated.svg);

    let (new_raw, _) = edupage::regenerate_artifact(
        &raw_file,
        artifact_id,
        &generated.svg,
        &generated.caption,
        new_aspect,
    )?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
fn delete_artifact(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    artifact_id: u32,
) -> Result<ChapterContent, String> {
    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let new_raw = edupage::delete_artifact(&raw, artifact_id)?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;
    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
async fn rewrite_passage(
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
    let messages = rewrite::build_rewrite_messages(&selection, &context, reading_level, topic);
    let raw_llm = dispatch_llm(&settings, messages).await?;
    let replacement = rewrite::clean_rewrite_response(&raw_llm);
    if replacement.is_empty() {
        return Err("model returned an empty rewrite".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) =
        edupage::rewrite_passage(&raw_file, &selection, occurrence_index, &replacement)?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[tauri::command]
async fn converse_about_rewrite(
    state: tauri::State<'_, AppState>,
    passage: String,
    context: String,
    history: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    if passage.trim().is_empty() {
        return Err("empty passage".to_string());
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
    let messages =
        rewrite::build_conversation_messages(&passage, &context, reading_level, topic, history);
    dispatch_llm(&settings, messages).await
}

#[tauri::command]
async fn rewrite_with_conversation(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    rewrite_id: u32,
    passage: String,
    context: String,
    history: Vec<llm::LlmMessage>,
) -> Result<ChapterContent, String> {
    if passage.trim().is_empty() {
        return Err("empty passage".to_string());
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
    let messages = rewrite::build_conversation_rewrite_messages(
        &passage,
        &context,
        reading_level,
        topic,
        history,
    );
    let raw_llm = dispatch_llm(&settings, messages).await?;
    let replacement = rewrite::clean_rewrite_response(&raw_llm);
    if replacement.is_empty() {
        return Err("model returned an empty rewrite".to_string());
    }

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::rewrite_existing_span(&raw_file, rewrite_id, &replacement)?;
    std::fs::write(&chapter_path, &new_raw)
        .map_err(|e| format!("failed to write chapter: {e}"))?;

    let page = edupage::read(&new_raw)?;
    Ok(ChapterContent::from_page(page))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendixResult {
    pub content: String,
    pub notes: Vec<edupage::NoteWithBody>,
    pub artifacts: Vec<edupage::ArtifactWithBody>,
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
        artifacts: page.artifacts,
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
    Ok(ChapterContent::from_page(page))
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
    Ok(ChapterContent::from_page(page))
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
fn scan_markdown_directory(path: String) -> Result<Vec<String>, String> {
    import::scan_markdown(std::path::Path::new(&path))
}

#[tauri::command]
fn import_book(
    state: tauri::State<'_, AppState>,
    source_dir: String,
    dest_path: String,
    ordered_files: Vec<String>,
) -> Result<manifest::Manifest, String> {
    let book = import::import_book(
        std::path::Path::new(&source_dir),
        std::path::Path::new(&dest_path),
        &ordered_files,
    )?;

    // Mirror create_book: derive the actual manifest path from dest_path stem
    // and store it on the AppState so subsequent commands resolve chapters.
    let stem = std::path::Path::new(&dest_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid destination path")?
        .to_string();
    let manifest_path = std::path::Path::new(&dest_path)
        .parent()
        .ok_or("invalid destination path")?
        .join(&stem)
        .join(format!("{stem}.edubook"));
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

            // 1. Persisted non-secret config (provider, Ollama, image quality).
            if let Ok(dir) = app.path().app_config_dir() {
                if let Some(cfg) = config::load_config(&dir) {
                    settings.apply_config(&cfg);
                }
            }

            // 2. Claude key from the OS keyring.
            settings.claude_api_key = config::keyring_get();

            // 3. Deprecated bridge: migrate ANTHROPIC_API_KEY into the keyring
            //    once, when nothing is stored yet. Afterwards the keyring is the
            //    source of truth and the env var is ignored.
            if settings.claude_api_key.is_none() {
                if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                    if !key.is_empty() {
                        let _ = config::keyring_set(&key);
                        settings.claude_api_key = Some(key);
                    }
                }
            }

            // 4. CLI flags remain a dev override layered on top (not persisted).
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

            app.manage(AppState {
                settings: std::sync::Mutex::new(settings),
                book_path: std::sync::Mutex::new(None),
            });

            Ok(())
        })
        .menu(|handle| {
            // Preserve the platform default menu (Edit copy/paste, etc.) and
            // add a Settings… item to the app submenu (⌘, on macOS).
            let settings_item =
                MenuItem::with_id(handle, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            let menu = Menu::default(handle)?;
            if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
                app_menu.insert(&settings_item, 1)?;
            }
            Ok(menu)
        })
        .on_menu_event(|app, event| {
            if event.id().0 == "settings" {
                let _ = app.emit("open-settings", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            call_llm,
            begin_onboarding,
            continue_onboarding,
            generate_lesson_plan,
            create_book,
            load_book,
            scan_markdown_directory,
            import_book,
            generate_chapter,
            read_chapter,
            define_word,
            add_footnote,
            add_endnote,
            add_appendix,
            add_note,
            delete_note,
            rewrite_passage,
            converse_about_rewrite,
            rewrite_with_conversation,
            add_image,
            add_diagram,
            delete_artifact,
            regenerate_artifact,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
