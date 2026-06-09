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
mod render;
mod rewrite;
mod settings;

use settings::{ImageQuality, LlmProvider, PublicSettings, Settings};
use tauri::menu::{Menu, MenuItem, MenuItemKind};
use tauri::{Emitter, Manager};
use tauri_plugin_cli::CliExt;

pub struct AppState {
    pub settings: std::sync::Mutex<Settings>,
    pub book_path: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// Handle to the `say` child process while it is speaking (macOS only).
    /// Wrapped in a Mutex so it can be killed from any command.
    pub say_process: std::sync::Mutex<Option<std::process::Child>>,
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

// Text dispatch for image/diagram *generation* steps (composition, initial
// render, polish, improve). Routes through `settings.image_provider` — which
// may differ from the general `settings.provider` — using the matching
// image-specific model name.
async fn dispatch_gen_text(
    settings: &Settings,
    messages: Vec<llm::LlmMessage>,
) -> Result<String, String> {
    match settings.image_provider {
        LlmProvider::Claude => {
            let key = settings
                .claude_api_key
                .as_ref()
                .ok_or("Claude API key not configured — set it in Settings (⌘,)")?;
            llm::call_claude(key, &settings.claude_image_model, messages).await
        }
        LlmProvider::Ollama => {
            llm::call_ollama(&settings.ollama_url, &settings.ollama_image_model, messages).await
        }
    }
}

// Vision dispatch for image/diagram *generation* steps (polish). Routes
// through `settings.image_provider`. Only Claude supports vision; Ollama
// returns an error so the polish loop falls back to the current candidate.
async fn dispatch_gen_vision(
    settings: &Settings,
    messages: Vec<llm::LlmMessage>,
    image: &llm::LlmImage,
) -> Result<String, String> {
    match settings.image_provider {
        LlmProvider::Claude => {
            let key = settings
                .claude_api_key
                .as_ref()
                .ok_or("Claude API key not configured")?;
            llm::call_claude_vision(key, &settings.claude_image_model, messages, image).await
        }
        LlmProvider::Ollama => {
            Err("vision polish is not supported for the Ollama provider".to_string())
        }
    }
}

// Vision dispatch for image/diagram *evaluation* steps (critique). The caller
// passes an explicit model (typically Haiku) so evaluation cost is kept
// separate from generation cost regardless of the image model setting.
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
            Err("vision critique is not supported for the Ollama provider".to_string())
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

    // Track the current best artifact as (body, mime_type).  SVG bodies are
    // text; raster bodies are base64-encoded bytes.
    let mut artifact: Option<(String, String)> = None;
    let mut critique: Option<String> = None;
    let mut pass: usize = 0;

    for step in &pipeline.steps {
        if step.critique_first {
            if let Some((body, mime)) = &artifact {
                let preview = image::preview_html(body, mime);
                emit_progress(app, "critique", pass, max, &preview);
                if let Ok(png) = image::to_png_bytes(body, mime) {
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
            let preview = artifact
                .as_ref()
                .map(|(b, m)| image::preview_html(b, m))
                .unwrap_or_default();
            emit_progress(app, "rendering", pass, max, &preview);

            if let Some((body, mime)) = artifact.clone() {
                let png = match image::to_png_bytes(&body, &mime) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("artifact pipeline: to_png_bytes failed, stopping polish: {e}");
                        break;
                    }
                };
                let llm_image = llm::LlmImage {
                    media_type: "image/png".to_string(),
                    base64_data: base64::engine::general_purpose::STANDARD.encode(&png),
                };
                let messages = build_polish(&composition, critique.as_deref());
                match dispatch_gen_vision(settings, messages, &llm_image).await {
                    Ok(resp) => match image::extract_image_response(&resp) {
                        Ok((new_mime, new_body)) => {
                            let preview = image::preview_html(&new_body, &new_mime);
                            artifact = Some((new_body, new_mime));
                            emit_progress(app, "rendering", pass, max, &preview);
                        }
                        Err(e) => {
                            eprintln!("artifact pipeline: polish returned no image: {e}");
                        }
                    },
                    Err(e) => {
                        eprintln!("artifact pipeline: polish call failed, stopping: {e}");
                        break;
                    }
                }
            } else {
                let messages = build_initial_render(&composition);
                let resp = dispatch_gen_text(settings, messages)
                    .await
                    .map_err(|e| { eprintln!("[polish loop] initial render call failed: {e}"); e })?;
                eprintln!(
                    "[polish loop] initial render response ({} chars): {:.200}",
                    resp.len(), resp,
                );
                let (mime, body) = image::extract_image_response(&resp)?;
                let preview = image::preview_html(&body, &mime);
                artifact = Some((body, mime));
                emit_progress(app, "rendering", pass, max, &preview);
            }
        }
    }

    let (body, mime_type) = artifact.ok_or("no image was produced")?;
    Ok(image::GeneratedImage { body, mime_type, caption })
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
    let composition = dispatch_gen_text(settings, comp_messages)
        .await
        .map_err(|e| { eprintln!("[image pipeline] composition call failed: {e}"); e })?
        .trim()
        .to_string();
    eprintln!(
        "[image pipeline] composition result ({} chars): {:.300}",
        composition.len(), composition,
    );

    // Diffusion-model fast-path: if the "composition" step returned image data
    // instead of prose (e.g. Flux, SDXL), skip the SVG polish loop entirely.
    // Re-run with a compact direct-generation prompt so the model is given a
    // clean description rather than the verbose composition phrasing.
    if image::extract_image_response(&composition).is_ok() {
        eprintln!("[image pipeline] composition returned image data — diffusion model path");
        emit_progress(app, "rendering", 1, 1, "");
        let direct = image::build_direct_gen_messages(source, context, extra_instruction);
        let resp = dispatch_gen_text(settings, direct)
            .await
            .map_err(|e| { eprintln!("[image pipeline] direct gen call failed: {e}"); e })?;
        eprintln!(
            "[image pipeline] direct gen result ({} chars): {:.200}",
            resp.len(), resp,
        );
        let (mime, body) = image::extract_image_response(&resp)?;
        let preview = image::preview_html(&body, &mime);
        emit_progress(app, "rendering", 1, 1, &preview);
        let caption = image::derive_caption(source);
        return Ok(image::GeneratedImage { body, mime_type: mime, caption });
    }

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

    // 1. Composition (image-gen model text).
    emit_progress(app, "composition", 0, max, "");
    let comp_messages =
        diagram::build_composition_messages(source, context, reading_level, original_prompt);
    let composition = dispatch_gen_text(settings, comp_messages)
        .await?
        .trim()
        .to_string();

    eprintln!(
        "[diagram pipeline] composition result ({} chars): {:.300}",
        composition.len(), composition,
    );

    // Diffusion-model fast-path (same as image pipeline): if composition
    // returned image data, re-run with a direct prompt and return immediately.
    // Diffusion models can't produce the SVG-based diagram/improve loop.
    if image::extract_image_response(&composition).is_ok() {
        eprintln!("[diagram pipeline] composition returned image data — diffusion model path");
        emit_progress(app, "rendering", 1, 1, "");
        let direct = image::build_direct_gen_messages(source, context, None);
        let resp = dispatch_gen_text(settings, direct).await?;
        let (mime, body) = image::extract_image_response(&resp)?;
        let preview = image::preview_html(&body, &mime);
        emit_progress(app, "rendering", 1, 1, &preview);
        let caption = image::derive_caption(source);
        return Ok(image::GeneratedImage { body, mime_type: mime, caption });
    }

    let caption = image::derive_caption(&composition);

    // 2. Initial render (image-gen model text).
    let mut pass: usize = 1;
    emit_progress(app, "rendering", pass, max, "");
    let init_messages = diagram::build_initial_render_messages(&composition);
    let resp = dispatch_gen_text(settings, init_messages).await?;
    let (mut mime, mut body) = image::extract_image_response(&resp)?;
    emit_progress(app, "rendering", pass, max, &image::preview_html(&body, &mime));

    // 3. Evaluate-and-improve loop (Haiku judges, image-gen model redraws).
    for _ in 0..diagram::MAX_ITERATIONS {
        let preview = image::preview_html(&body, &mime);
        emit_progress(app, "critique", pass, max, &preview);

        let png = match image::to_png_bytes(&body, &mime) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("diagram pipeline: to_png failed, stopping: {e}");
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
        emit_progress(app, "rendering", pass, max, &image::preview_html(&body, &mime));
        // Improve prompt expects SVG; if we have raster only, skip improve.
        if mime != "image/svg+xml" {
            eprintln!("diagram pipeline: improve step skipped (model returned raster, not SVG)");
            break;
        }
        let improve_messages = diagram::build_improve_messages(&composition, &body, &score);
        match dispatch_gen_text(settings, improve_messages).await {
            Ok(resp) => match image::extract_image_response(&resp) {
                Ok((new_mime, new_body)) => {
                    mime = new_mime;
                    body = new_body;
                    emit_progress(app, "rendering", pass, max, &image::preview_html(&body, &mime));
                }
                Err(e) => {
                    eprintln!("diagram pipeline: improve returned no image, keeping current: {e}");
                    break;
                }
            },
            Err(e) => {
                eprintln!("diagram pipeline: improve call failed, keeping current: {e}");
                break;
            }
        }
    }

    Ok(image::GeneratedImage { body, mime_type: mime, caption })
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
    image_provider: String,
    claude_image_model: String,
    ollama_image_model: String,
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
    let image_provider = match update.image_provider.as_str() {
        "claude" => LlmProvider::Claude,
        "ollama" => LlmProvider::Ollama,
        other => return Err(format!("unknown image provider: {other}")),
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
    settings.image_provider = image_provider;
    settings.claude_image_model = update.claude_image_model;
    settings.ollama_image_model = update.ollama_image_model;
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
    /// Fully-rendered, source-annotated HTML produced by `render::render_chapter_html`.
    pub html: String,
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

    // Render immediately so the frontend receives HTML rather than raw markdown.
    let html = render::render_chapter_html(&content, &[], &[]);
    Ok(GenerateChapterResult { html, manifest: book })
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
    src_end: usize,
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
    let (new_raw, _) = edupage::add_note_at(
        &raw_file,
        edupage::NoteType::Definition,
        &word,
        src_end,
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
    /// Fully-rendered, source-annotated HTML from `render::render_chapter_html`.
    /// The frontend sets this directly as `innerHTML`; no further markdown
    /// processing is required.
    pub html: String,
    pub notes: Vec<edupage::NoteWithBody>,
    pub artifacts: Vec<edupage::ArtifactWithBody>,
}

impl ChapterContent {
    fn from_page(page: edupage::EduPage) -> Self {
        let html = render::render_chapter_html(&page.content, &page.notes, &page.artifacts);
        ChapterContent {
            html,
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
    selection_text: String,
    src_end: usize,
    context: String,
) -> Result<ChapterContent, String> {
    let selection = &selection_text;
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
    let (new_raw, _) = edupage::add_note_at(
        &raw_file,
        edupage::NoteType::Footnote,
        selection,
        src_end,
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
    selection_text: String,
    src_end: usize,
    context: String,
) -> Result<ChapterContent, String> {
    let selection = &selection_text;
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
    let (new_raw, _) = edupage::add_note_at(
        &raw_file,
        edupage::NoteType::Endnote,
        selection,
        src_end,
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
    selection_text: String,
    src_start: usize,
    src_end: usize,
    context: String,
) -> Result<ChapterContent, String> {
    if selection_text.trim().is_empty() {
        return Err("empty selection".to_string());
    }
    if context.trim().is_empty() {
        return Err("empty context".to_string());
    }

    let settings = state.settings.lock().unwrap().clone();
    let generated =
        run_image_pipeline(&app, &settings, &selection_text, &context, None).await?;
    let aspect = image::aspect_ratio_for(&generated.body, &generated.mime_type);

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_artifact_at(
        &raw_file,
        src_start,
        src_end,
        &generated.body,
        &generated.mime_type,
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
    selection_text: String,
    src_start: usize,
    src_end: usize,
    context: String,
) -> Result<ChapterContent, String> {
    if selection_text.trim().is_empty() {
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
    let original_prompt = Some(book.metadata.prompt.as_str()).filter(|s| !s.is_empty());

    let settings = state.settings.lock().unwrap().clone();
    let generated = run_diagram_pipeline(
        &app,
        &settings,
        &selection_text,
        &context,
        reading_level,
        original_prompt,
    )
    .await?;
    let aspect = image::aspect_ratio_for(&generated.body, &generated.mime_type);

    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw_file = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;
    let (new_raw, _) = edupage::add_artifact_at(
        &raw_file,
        src_start,
        src_end,
        &generated.body,
        &generated.mime_type,
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
    let new_aspect = image::aspect_ratio_for(&generated.body, &generated.mime_type);

    let (new_raw, _) = edupage::regenerate_artifact(
        &raw_file,
        artifact_id,
        &generated.body,
        &generated.mime_type,
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
    selection_text: String,
    src_start: usize,
    src_end: usize,
    context: String,
) -> Result<ChapterContent, String> {
    let selection = &selection_text;
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
        edupage::rewrite_passage_at(&raw_file, src_start, src_end, &replacement)?;
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
    pub html: String,
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
    selection_text: String,
    src_end: usize,
    context: String,
) -> Result<AppendixResult, String> {
    let selection = &selection_text;
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
        edupage::insert_appendix_ref_at(&source_raw, seq, src_end)?;
    std::fs::write(&source_path, &new_source_raw)
        .map_err(|e| format!("failed to write source chapter: {e}"))?;

    let page = edupage::read(&new_source_raw)?;
    let html = render::render_chapter_html(&page.content, &page.notes, &page.artifacts);
    Ok(AppendixResult {
        html,
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
    src_end: usize,
    body: String,
) -> Result<ChapterContent, String> {
    let chapter_path = chapter_path_for(&state, &chapter_id)?;
    let raw = std::fs::read_to_string(&chapter_path)
        .map_err(|e| format!("failed to read chapter: {e}"))?;

    let nt = match note_type.as_str() {
        "definition" => edupage::NoteType::Definition,
        other => return Err(format!("unknown note type: {other}")),
    };

    let (new_raw, _) = edupage::add_note_at(&raw, nt, &word, src_end, &body)?;

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
    let manifest_path = book_dir.join(format!("{stem}.scholium"));

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
        .join(format!("{stem}.scholium"));
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

// ── Book chat ─────────────────────────────────────────────────────────────────

/// One message in the chat history as seen by the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// What the backend returns for each chat turn.
#[derive(Debug, serde::Serialize)]
pub struct ChatReply {
    /// The assistant's reply text (may contain a ```json patch block).
    pub reply: String,
    /// If the reply contained a parseable patch block, the parsed action is
    /// echoed back here so the frontend can inspect it without re-parsing.
    pub patch: Option<ManifestPatch>,
}

/// A structured action the AI can propose for the book.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ManifestPatch {
    RenameChapter { id: String, title: String },
    ReorderChapters { ids: Vec<String> },
    AddChapter { after_id: Option<String>, id: String, title: String, description: Option<String> },
    RemoveChapter { id: String },
    MergeChapters { source_id: String, target_id: String, title: String },
    SplitChapter { id: String, new_chapters: Vec<NewChapterSpec> },
    UpdateMetadata { title: Option<String>, subtitle: Option<String>, description: Option<String> },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewChapterSpec {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
}

/// Build the system prompt for the book chat, giving the AI full context.
fn chat_system_prompt(book: &manifest::Manifest) -> String {
    let chapters: Vec<String> = book
        .lesson_plan
        .chapters
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let desc = ch
                .description
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            let status = match ch.status {
                manifest::ChapterStatus::Planned => "planned",
                manifest::ChapterStatus::Generating => "generating",
                manifest::ChapterStatus::Generated => "generated",
            };
            format!("  {}. [{}] {} ({}){}", i + 1, ch.id, ch.title, status, desc)
        })
        .collect();

    format!(
        r#"You are a helpful assistant for an interactive educational book.

Book: {title}
Topic: {topic}
{subtitle_line}Summary: {summary}

Chapters:
{chapter_list}

You can chat freely to help the reader understand the content, or you can propose
structural changes to the book. When you want to propose a change, include a JSON
block anywhere in your reply using this format:

```json
{{ "action": "<action_name>", ... }}
```

Supported actions and their fields:
- rename_chapter:   {{ "action": "rename_chapter",   "id": "ch-01", "title": "New Title" }}
- reorder_chapters: {{ "action": "reorder_chapters",  "ids": ["ch-02", "ch-01", "ch-03"] }}
- add_chapter:      {{ "action": "add_chapter",       "after_id": "ch-01" | null, "id": "ch-new", "title": "…", "description": "…" | null }}
- remove_chapter:   {{ "action": "remove_chapter",    "id": "ch-01" }}
- merge_chapters:   {{ "action": "merge_chapters",    "source_id": "ch-02", "target_id": "ch-01", "title": "Merged Title" }}
- split_chapter:    {{ "action": "split_chapter",     "id": "ch-01", "new_chapters": [{{"id":"ch-01a","title":"…","description":null}}] }}
- update_metadata:  {{ "action": "update_metadata",   "title": "…" | null, "subtitle": "…" | null, "description": "…" | null }}

Only include a JSON block when you are actively proposing a change. The user will
be shown the proposed change and can accept or reject it before anything is applied.
Omit the block for informational replies."#,
        title = book.metadata.title,
        topic = book.metadata.topic,
        subtitle_line = book
            .metadata
            .subtitle
            .as_deref()
            .map(|s| format!("Subtitle: {s}\n"))
            .unwrap_or_default(),
        summary = book.lesson_plan.summary,
        chapter_list = chapters.join("\n"),
    )
}

/// Extract the first ```json ... ``` block from the assistant reply and try to
/// parse it as a ManifestPatch.
fn extract_patch(reply: &str) -> Option<ManifestPatch> {
    let start = reply.find("```json")?;
    let after = &reply[start + 7..];
    let end = after.find("```")?;
    let json_str = after[..end].trim();
    serde_json::from_str(json_str).ok()
}

#[tauri::command]
async fn chat_about_book(
    state: tauri::State<'_, AppState>,
    history: Vec<ChatMessage>,
    // Optional highlighted passage the user has selected in the book.
    // When present it is appended to the system prompt so the AI knows
    // exactly what the user is looking at.
    highlight: Option<String>,
) -> Result<ChatReply, String> {
    let settings = state.settings.lock().unwrap().clone();
    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("No book is open")?;
    let book = manifest::load(&book_path)?;
    let mut system_prompt = chat_system_prompt(&book);

    if let Some(ref passage) = highlight {
        system_prompt.push_str(&format!(
            "\n\nThe user has highlighted the following passage in the book:\n\n\
             \"{passage}\"\n\n\
             Refer to this passage when answering unless the user's question is clearly unrelated to it."
        ));
    }

    // Build the full message list: system + conversation history.
    let mut messages = vec![llm::LlmMessage {
        role: "system".to_string(),
        content: system_prompt,
    }];
    for msg in history {
        messages.push(llm::LlmMessage { role: msg.role, content: msg.content });
    }

    let reply = dispatch_llm(&settings, messages).await?;
    let patch = extract_patch(&reply);
    Ok(ChatReply { reply, patch })
}

#[tauri::command]
fn apply_manifest_patch(
    state: tauri::State<'_, AppState>,
    patch: ManifestPatch,
) -> Result<manifest::Manifest, String> {
    let book_path = state
        .book_path
        .lock()
        .unwrap()
        .clone()
        .ok_or("No book is open")?;
    let mut book = manifest::load(&book_path)?;

    match patch {
        ManifestPatch::RenameChapter { id, title } => {
            let ch = book
                .lesson_plan
                .chapters
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| format!("chapter '{id}' not found"))?;
            ch.title = title;
        }

        ManifestPatch::ReorderChapters { ids } => {
            let mut reordered: Vec<manifest::Chapter> = Vec::with_capacity(ids.len());
            for id in &ids {
                let pos = book
                    .lesson_plan
                    .chapters
                    .iter()
                    .position(|c| &c.id == id)
                    .ok_or_else(|| format!("chapter '{id}' not found"))?;
                reordered.push(book.lesson_plan.chapters[pos].clone());
            }
            book.lesson_plan.chapters = reordered;
        }

        ManifestPatch::AddChapter { after_id, id, title, description } => {
            let new_ch = manifest::Chapter {
                id,
                title,
                description,
                file: String::new(), // placeholder; generation will fill this
                status: manifest::ChapterStatus::Planned,
            };
            match after_id {
                None => book.lesson_plan.chapters.insert(0, new_ch),
                Some(aid) => {
                    let pos = book
                        .lesson_plan
                        .chapters
                        .iter()
                        .position(|c| c.id == aid)
                        .ok_or_else(|| format!("chapter '{aid}' not found"))?;
                    book.lesson_plan.chapters.insert(pos + 1, new_ch);
                }
            }
        }

        ManifestPatch::RemoveChapter { id } => {
            let pos = book
                .lesson_plan
                .chapters
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| format!("chapter '{id}' not found"))?;
            book.lesson_plan.chapters.remove(pos);
        }

        ManifestPatch::MergeChapters { source_id, target_id, title } => {
            // Remove source; rename target to the merged title.
            let src_pos = book
                .lesson_plan
                .chapters
                .iter()
                .position(|c| c.id == source_id)
                .ok_or_else(|| format!("chapter '{source_id}' not found"))?;
            book.lesson_plan.chapters.remove(src_pos);
            let tgt = book
                .lesson_plan
                .chapters
                .iter_mut()
                .find(|c| c.id == target_id)
                .ok_or_else(|| format!("chapter '{target_id}' not found"))?;
            tgt.title = title;
            tgt.status = manifest::ChapterStatus::Planned; // needs regeneration
        }

        ManifestPatch::SplitChapter { id, new_chapters } => {
            let pos = book
                .lesson_plan
                .chapters
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| format!("chapter '{id}' not found"))?;
            book.lesson_plan.chapters.remove(pos);
            for (i, spec) in new_chapters.into_iter().enumerate() {
                book.lesson_plan.chapters.insert(
                    pos + i,
                    manifest::Chapter {
                        id: spec.id,
                        title: spec.title,
                        description: spec.description,
                        file: String::new(),
                        status: manifest::ChapterStatus::Planned,
                    },
                );
            }
        }

        ManifestPatch::UpdateMetadata { title, subtitle, description } => {
            if let Some(t) = title {
                book.metadata.title = t;
            }
            if let Some(s) = subtitle {
                book.metadata.subtitle = Some(s);
            }
            if let Some(d) = description {
                book.metadata.description = Some(d);
            }
        }
    }

    // Stamp modified timestamp.
    book.metadata.modified = chrono::Utc::now().to_rfc3339();
    manifest::save(&book, &book_path)?;
    Ok(book)
}

// ── Text-to-speech ────────────────────────────────────────────────────────────

/// Speak `text` aloud.
///
/// On macOS the system `say` command is used (piping text via stdin so there
/// is no argument-length limit).  Any previously-running `say` process is
/// killed first, making this safe to call repeatedly.
///
/// On other platforms this command returns `Err("platform_not_supported")` so
/// the frontend can fall back to the Web Speech API.
#[tauri::command]
fn speak_text(state: tauri::State<'_, AppState>, text: String) -> Result<(), String> {
    // Kill any in-progress say process (no-op on non-macOS where the mutex is
    // always None).
    {
        let mut lock = state.say_process.lock().map_err(|e| e.to_string())?;
        if let Some(mut child) = lock.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        return Err("platform_not_supported".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        use std::io::Write;
        let mut child = std::process::Command::new("say")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start say: {e}"))?;

        // Write text to `say`'s stdin in a background thread so this command
        // returns immediately.  When the thread finishes writing, the pipe is
        // closed and `say` knows it has received all input.
        if let Some(mut stdin) = child.stdin.take() {
            let bytes = text.into_bytes();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }

        *state.say_process.lock().map_err(|e| e.to_string())? = Some(child);
        Ok(())
    }
}

/// Stop any in-progress speech started by `speak_text`.
/// On non-macOS this is a no-op (the frontend's Web Speech cancel is handled
/// client-side).
#[tauri::command]
fn stop_speaking(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut lock = state.say_process.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = lock.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
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
                say_process: std::sync::Mutex::new(None),
            });

            Ok(())
        })
        .menu(|handle| {
            // Preserve the platform default menu (Edit copy/paste, etc.) and
            // add Settings… and Chat… items to the app submenu.
            let settings_item =
                MenuItem::with_id(handle, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
            let chat_item =
                MenuItem::with_id(handle, "chat", "Chat…", true, Some("CmdOrCtrl+K"))?;
            let read_item =
                MenuItem::with_id(handle, "read-aloud", "Read Aloud", true, Some("CmdOrCtrl+Shift+R"))?;
            let menu = Menu::default(handle)?;
            if let Some(MenuItemKind::Submenu(app_menu)) = menu.items()?.into_iter().next() {
                app_menu.insert(&settings_item, 1)?;
                app_menu.insert(&chat_item, 2)?;
                app_menu.insert(&read_item, 3)?;
            }
            Ok(menu)
        })
        .on_menu_event(|app, event| {
            match event.id().0.as_str() {
                "settings" => { let _ = app.emit("open-settings", ()); }
                "chat" => { let _ = app.emit("open-chat", ()); }
                "read-aloud" => { let _ = app.emit("start-reading", ()); }
                _ => {}
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
            chat_about_book,
            apply_manifest_patch,
            speak_text,
            stop_speaking,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
