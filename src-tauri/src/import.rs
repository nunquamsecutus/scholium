//! Import a directory of markdown files into an edu-harness book.

use crate::manifest::{Chapter, ChapterStatus, LessonPlan, Manifest, Metadata};
use crate::onboarding::slugify;
use crate::{edupage, manifest};
use std::path::Path;

/// Recognized markdown file extensions (case-insensitive).
const MARKDOWN_EXTS: &[&str] = &["md", "markdown"];

/// Return the basenames of markdown files in `dir`, sorted alphabetically.
pub fn scan_markdown(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("failed to read directory: {e}"))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let path = e.path();
            let ext = path.extension().and_then(|x| x.to_str()).map(|s| s.to_ascii_lowercase());
            match ext {
                Some(ext) if MARKDOWN_EXTS.contains(&ext.as_str()) => path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string()),
                _ => None,
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

fn title_case(s: &str) -> String {
    let mut out = String::new();
    let mut next_upper = true;
    for c in s.chars() {
        if c.is_whitespace() {
            out.push(c);
            next_upper = true;
        } else if next_upper {
            for u in c.to_uppercase() {
                out.push(u);
            }
            next_upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert a markdown filename ("01-stellar-evolution.md") into a chapter
/// title ("01 Stellar Evolution") by stripping the extension, replacing
/// separators with spaces, and title-casing words.
pub fn chapter_title_from_filename(filename: &str) -> String {
    let stem = filename
        .strip_suffix(".markdown")
        .or_else(|| filename.strip_suffix(".md"))
        .or_else(|| filename.strip_suffix(".MD"))
        .or_else(|| filename.strip_suffix(".MARKDOWN"))
        .unwrap_or(filename);
    let spaced = stem.replace(['-', '_', '.'], " ");
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    title_case(&collapsed)
}

/// Derive a book title from a source directory name.
pub fn book_title_from_dirname(name: &str) -> String {
    let spaced = name.replace(['-', '_', '.'], " ");
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    title_case(&collapsed)
}

/// Build the book file layout at `dest_path` from `source_dir` and the
/// caller-provided chapter order. Returns the loaded manifest.
///
/// `dest_path` is the user-chosen `.edubook` save location (mirroring the
/// create_book flow). Each markdown file becomes an `.edupage` under
/// `chapters/`; a manifest is written at `dest_path`.
pub fn import_book(
    source_dir: &Path,
    dest_path: &Path,
    ordered_files: &[String],
) -> Result<Manifest, String> {
    if ordered_files.is_empty() {
        return Err("no chapters to import".to_string());
    }

    let stem = dest_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid destination path")?
        .to_string();
    let book_dir = dest_path.parent().ok_or("invalid destination path")?.join(&stem);
    let manifest_path = book_dir.join(format!("{stem}.edubook"));

    std::fs::create_dir_all(book_dir.join("chapters"))
        .map_err(|e| format!("failed to create chapters dir: {e}"))?;
    std::fs::create_dir_all(book_dir.join("images"))
        .map_err(|e| format!("failed to create images dir: {e}"))?;

    let mut chapters: Vec<Chapter> = Vec::with_capacity(ordered_files.len());
    for (idx, filename) in ordered_files.iter().enumerate() {
        let n = idx + 1;
        let title = chapter_title_from_filename(filename);
        let slug = slugify(filename.trim_end_matches(".markdown").trim_end_matches(".md"));
        let chapter_id = format!("ch-{}", n);
        let chapter_file = format!("chapters/{}-{}.edupage", n, slug);

        let source_path = source_dir.join(filename);
        let content = std::fs::read_to_string(&source_path)
            .map_err(|e| format!("failed to read {filename}: {e}"))?;

        let edupage_raw = edupage::create(&chapter_id, &title, None, &content);
        std::fs::write(book_dir.join(&chapter_file), &edupage_raw)
            .map_err(|e| format!("failed to write chapter {filename}: {e}"))?;

        chapters.push(Chapter {
            id: chapter_id,
            title,
            description: None,
            file: chapter_file,
            status: ChapterStatus::Generated,
        });
    }

    let now = chrono::Utc::now().to_rfc3339();
    let book_title = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(book_title_from_dirname)
        .unwrap_or_else(|| stem.clone());

    let book = Manifest {
        version: 1,
        metadata: Metadata {
            title: book_title.clone(),
            subtitle: Some("Imported".to_string()),
            topic: book_title,
            prompt: String::new(),
            created: now.clone(),
            modified: now,
            description: None,
            reading_level: None,
            prior_knowledge: None,
        },
        lesson_plan: LessonPlan {
            summary: String::new(),
            chapters,
        },
    };

    manifest::save(&book, &manifest_path)?;
    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("edu-harness-import-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn chapter_title_strips_extension_and_title_cases() {
        assert_eq!(chapter_title_from_filename("01-intro.md"), "01 Intro");
        assert_eq!(
            chapter_title_from_filename("stellar_evolution.markdown"),
            "Stellar Evolution"
        );
        assert_eq!(chapter_title_from_filename("hello.MD"), "Hello");
    }

    #[test]
    fn scan_returns_only_markdown_sorted() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("02-second.md"), "two").unwrap();
        std::fs::write(dir.join("01-first.md"), "one").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        std::fs::write(dir.join("README.MARKDOWN"), "also md").unwrap();
        let names = scan_markdown(&dir).unwrap();
        assert_eq!(names, vec!["01-first.md", "02-second.md", "README.MARKDOWN"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_errors_on_missing_directory() {
        assert!(scan_markdown(Path::new("/definitely/not/a/path/here")).is_err());
    }

    #[test]
    fn import_book_creates_manifest_and_chapter_files() {
        let src = temp_dir();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.md"), "# A\nFirst chapter body.").unwrap();
        std::fs::write(src.join("b.md"), "# B\nSecond chapter body.").unwrap();

        let book_root = temp_dir();
        std::fs::create_dir_all(&book_root).unwrap();
        let dest = book_root.join("my-book.edubook");

        let order = vec!["b.md".to_string(), "a.md".to_string()];
        let manifest = import_book(&src, &dest, &order).unwrap();

        // Manifest lists chapters in the user-supplied order.
        assert_eq!(manifest.lesson_plan.chapters.len(), 2);
        assert_eq!(manifest.lesson_plan.chapters[0].title, "B");
        assert_eq!(manifest.lesson_plan.chapters[1].title, "A");
        assert!(matches!(
            manifest.lesson_plan.chapters[0].status,
            ChapterStatus::Generated
        ));

        // Each chapter file exists and round-trips back to its original body.
        let book_dir = book_root.join("my-book");
        for ch in &manifest.lesson_plan.chapters {
            let raw = std::fs::read_to_string(book_dir.join(&ch.file)).unwrap();
            let body = edupage::reconstruct(&raw).unwrap();
            assert!(body.starts_with(&format!("# {}", ch.title)));
        }

        // Manifest itself is on disk where we expect.
        assert!(book_dir.join("my-book.edubook").exists());

        std::fs::remove_dir_all(&src).ok();
        std::fs::remove_dir_all(&book_root).ok();
    }
}
