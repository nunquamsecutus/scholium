import { createSignal, createEffect, createMemo, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import DOMPurify from "dompurify";
import type { Chapter, Manifest } from "../types/manifest";
import SelectionToolbar from "./SelectionToolbar";

// Pagination unit comes from the article's clientWidth at runtime — see the
// page-window / chapter-content split in App.css. The browser fits one column
// to the available content area, and clientWidth gives us that width exactly,
// so scrollLeft and getBoundingClientRect math stay aligned without a brittle
// hardcoded constant.

interface GenerateResult {
  content: string;
  manifest: Manifest;
}

interface NoteFromBackend {
  id: number;
  type: string;
  word: string;
  ctime: string;
  body: string;
}

interface ChapterContent {
  content: string;
  notes: NoteFromBackend[];
}

interface Props {
  manifest: Manifest;
}

function displayForNote(type: string | undefined, id: string): string {
  switch (type) {
    case "definition":
      return "📖";
    case "footnote":
    case "endnote":
      return id;
    default:
      return "📖";
  }
}

function labelForNote(note: NoteFromBackend | undefined, id: string): string {
  if (!note) return `note ${id}`;
  if (note.type === "footnote") return `Footnote ${id}`;
  if (note.type === "endnote") return `Endnote ${id}`;
  return `${note.type} of ${note.word}`;
}

// `[^*N]` = definition, `[^†N]` = footnote, `[^‡N]` = endnote. Future
// appendix marker (`A`) will slot in here.
const ANCHOR_RE = /\[\^([*†‡])(\d+)\]/g;

function renderMarkdown(md: string, notes: NoteFromBackend[]): string {
  const html = DOMPurify.sanitize(marked.parse(md) as string);
  const byId = new Map<number, NoteFromBackend>(notes.map((n) => [n.id, n]));
  let processed = html.replace(ANCHOR_RE, (_, _marker, id) => {
    const note = byId.get(Number(id));
    const type = note?.type ?? "unknown";
    const display = displayForNote(note?.type, id);
    const label = labelForNote(note, id);
    return `<sup class="note-anchor" data-note-id="${id}" data-note-type="${type}" aria-label="${label}">${display}</sup>`;
  });

  // Append an "Endnotes" section at the end of the chapter for any endnotes
  // present. It flows through the same column layout as the main content, so
  // it lands on whichever page it falls on.
  const endnotes = notes.filter((n) => n.type === "endnote");
  if (endnotes.length > 0) {
    let section = '<hr class="endnotes-rule" /><section class="endnotes" aria-label="Endnotes"><h2 class="endnotes-heading">Endnotes</h2><ol class="endnotes-list">';
    for (const en of endnotes) {
      const body = DOMPurify.sanitize(marked.parse(en.body) as string);
      section += `<li id="endnote-${en.id}" data-endnote-target="${en.id}" class="endnote-item"><span class="endnote-number">${en.id}.</span><div class="endnote-body">${body}</div></li>`;
    }
    section += "</ol></section>";
    processed += section;
  }

  return processed;
}

function isWordChar(c: string): boolean {
  return /[\p{L}\p{N}'\-]/u.test(c);
}

// Returns the text of the closest block-level ancestor (paragraph, list item,
// heading, blockquote) of `range`'s start, truncated to `max` characters.
// Falls back to the container's text if no block ancestor is found. Used to
// give the LLM enough surrounding context to pick the right sense of a word.
export function paragraphContext(range: Range, container: HTMLElement, max = 1000): string {
  const BLOCK_TAGS = new Set(["P", "LI", "BLOCKQUOTE", "H1", "H2", "H3", "H4", "H5", "H6"]);
  let node: Node | null = range.startContainer;
  while (node && node !== container) {
    if (node.nodeType === Node.ELEMENT_NODE && BLOCK_TAGS.has((node as HTMLElement).tagName)) {
      const text = (node as HTMLElement).textContent ?? "";
      return text.length > max ? text.slice(0, max) : text;
    }
    node = node.parentNode;
  }
  return (container.textContent ?? "").slice(0, max);
}

// Returns the 1-based occurrence index of `word` (word-bounded) covering the
// start of `range` within `container`, walking text nodes individually so
// element boundaries are treated as word boundaries (matching the backend's
// line-based scan of the markdown source). Returns -1 if not found.
export function occurrenceIndex(range: Range, container: HTMLElement, word: string): number {
  let count = 0;
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  while (node) {
    const text = node.textContent ?? "";
    const isTarget = node === range.startContainer;
    let i = 0;
    while (i <= text.length - word.length) {
      if (text.substring(i, i + word.length) === word) {
        const before = i === 0 ? "" : text[i - 1];
        const after = i + word.length >= text.length ? "" : text[i + word.length];
        if (!isWordChar(before) && !isWordChar(after)) {
          count++;
          if (isTarget && i <= range.startOffset && range.startOffset <= i + word.length) {
            return count;
          }
        }
      }
      i++;
    }
    if (isTarget) return -1;
    node = walker.nextNode();
  }
  return -1;
}

export default function BookView(props: Props) {
  const [manifest, setManifest] = createSignal(props.manifest);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [contentCache, setContentCache] = createSignal<Record<string, string>>({});
  const [chapterNotes, setChapterNotes] = createSignal<Record<string, NoteFromBackend[]>>({});
  const [notePages, setNotePages] = createSignal<Record<number, number>>({});
  const [activeFootnote, setActiveFootnote] = createSignal<NoteFromBackend | null>(null);
  const [endnoteReturnPage, setEndnoteReturnPage] = createSignal<number | null>(null);
  const [generating, setGenerating] = createSignal(false);
  const [error, setError] = createSignal("");
  const [articleRef, setArticleRef] = createSignal<HTMLElement>();
  const [currentPage, setCurrentPage] = createSignal(0);
  const [pageCount, setPageCount] = createSignal(1);

  const selectedChapter = () =>
    manifest().lessonPlan.chapters.find((ch) => ch.id === selectedId()) ?? null;

  const nextPlannedId = () =>
    manifest().lessonPlan.chapters.find((ch) => ch.status === "planned")?.id ?? null;

  const currentHtml = () =>
    selectedId() ? contentCache()[selectedId()!] ?? null : null;

  const currentNotes = () => {
    const id = selectedId();
    return id ? chapterNotes()[id] ?? [] : [];
  };

  const visibleMarginalia = createMemo(() => {
    const notes = currentNotes();
    const pages = notePages();
    const page = currentPage();
    return notes
      .filter((n) => pages[n.id] === page)
      .map((n) => ({ id: n.id, word: n.word, body: n.body }));
  });

  function recomputePageCount() {
    const article = articleRef();
    if (!article) return;
    const colWidth = article.clientWidth;
    if (colWidth === 0) return;
    const count = Math.max(1, Math.ceil(article.scrollWidth / colWidth));
    setPageCount(count);
    if (currentPage() >= count) setCurrentPage(count - 1);
  }

  function pageOfElement(el: HTMLElement): number {
    const article = articleRef();
    if (!article) return 0;
    const colWidth = article.clientWidth;
    if (colWidth === 0) return 0;
    const elRect = el.getBoundingClientRect();
    const aRect = article.getBoundingClientRect();
    const offsetLeft = elRect.left - aRect.left + article.scrollLeft;
    return Math.max(0, Math.floor(offsetLeft / colWidth));
  }

  function recomputeNotePages() {
    const article = articleRef();
    if (!article) return;
    const notes = currentNotes();
    const pages: Record<number, number> = {};
    for (const n of notes) {
      const sup = article.querySelector(`[data-note-id="${n.id}"]`) as HTMLElement | null;
      if (sup) pages[n.id] = pageOfElement(sup);
    }
    setNotePages(pages);
  }

  function prev() {
    setCurrentPage((p) => Math.max(0, p - 1));
  }
  function next() {
    setCurrentPage((p) => Math.min(pageCount() - 1, p + 1));
  }

  // Apply the current page by scrolling the column container.
  createEffect(() => {
    const article = articleRef();
    const page = currentPage();
    if (article) article.scrollLeft = page * article.clientWidth;
  });

  // Recompute pagination + note positions whenever chapter content or notes
  // change. Explicit currentNotes() read keeps tracking robust — without it,
  // a note added without an innerHTML change wouldn't trigger a recompute.
  createEffect(() => {
    const html = currentHtml();
    currentNotes();
    const article = articleRef();
    if (!html || !article) return;
    queueMicrotask(() => {
      recomputePageCount();
      recomputeNotePages();
    });
  });

  // Keyboard navigation. Escape closes the footnote sheet; otherwise arrows
  // turn pages.
  onMount(() => {
    const onKey = (e: KeyboardEvent) => {
      if (activeFootnote()) {
        if (e.key === "Escape") {
          setActiveFootnote(null);
          e.preventDefault();
        }
        return;
      }
      const ch = selectedChapter();
      if (!ch || ch.status !== "generated") return;
      if (e.target instanceof HTMLElement) {
        const tag = e.target.tagName;
        if (tag === "INPUT" || tag === "TEXTAREA") return;
      }
      if (e.key === "ArrowLeft") {
        prev();
        e.preventDefault();
      } else if (e.key === "ArrowRight") {
        next();
        e.preventDefault();
      }
    };
    document.addEventListener("keydown", onKey);
    onCleanup(() => document.removeEventListener("keydown", onKey));
  });

  function jumpToEndnote(noteId: number) {
    const article = articleRef();
    if (!article) return;
    const el = article.querySelector(
      `[data-endnote-target="${noteId}"]`,
    ) as HTMLElement | null;
    if (!el) return;
    setEndnoteReturnPage(currentPage());
    setCurrentPage(pageOfElement(el));
  }

  function returnFromEndnote() {
    const back = endnoteReturnPage();
    if (back !== null) {
      setCurrentPage(back);
      setEndnoteReturnPage(null);
    }
  }

  // Anchor clicks inside the article: footnotes open the bottom sheet,
  // endnotes jump to the endnotes section (remembering the return page).
  // Definitions don't take a click action yet (they live in the margin).
  createEffect(() => {
    const article = articleRef();
    if (!article) return;
    const onClick = (e: MouseEvent) => {
      const target = e.target as HTMLElement | null;
      const anchor = target?.closest(".note-anchor") as HTMLElement | null;
      if (!anchor) return;
      const noteId = Number(anchor.dataset.noteId);
      const noteType = anchor.dataset.noteType;
      if (noteType === "footnote") {
        e.preventDefault();
        const note = currentNotes().find((n) => n.id === noteId);
        if (note) setActiveFootnote(note);
      } else if (noteType === "endnote") {
        e.preventDefault();
        jumpToEndnote(noteId);
      }
    };
    article.addEventListener("click", onClick);
    onCleanup(() => article.removeEventListener("click", onClick));
  });

  async function selectChapter(ch: Chapter) {
    setError("");
    setSelectedId(ch.id);
    setCurrentPage(0);
    setEndnoteReturnPage(null);

    if (ch.status === "generated" && !contentCache()[ch.id]) {
      try {
        const result = await invoke<ChapterContent>("read_chapter", { chapterId: ch.id });
        setContentCache((c) => ({ ...c, [ch.id]: renderMarkdown(result.content, result.notes) }));
        setChapterNotes((m) => ({ ...m, [ch.id]: result.notes }));
      } catch (e) {
        setError(String(e));
      }
    }
  }

  async function generateChapter(chapterId: string) {
    setError("");
    setGenerating(true);

    setManifest((m) => ({
      ...m,
      lessonPlan: {
        ...m.lessonPlan,
        chapters: m.lessonPlan.chapters.map((ch) =>
          ch.id === chapterId ? { ...ch, status: "generating" as const } : ch,
        ),
      },
    }));

    try {
      const result = await invoke<GenerateResult>("generate_chapter", { chapterId });
      setManifest(result.manifest);
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content, []) }));
      setChapterNotes((m) => ({ ...m, [chapterId]: [] }));
      setSelectedId(chapterId);
      setCurrentPage(0);
    } catch (e) {
      setError(String(e));
      setManifest((m) => ({
        ...m,
        lessonPlan: {
          ...m.lessonPlan,
          chapters: m.lessonPlan.chapters.map((ch) =>
            ch.id === chapterId ? { ...ch, status: "planned" as const } : ch,
          ),
        },
      }));
    } finally {
      setGenerating(false);
    }
  }

  async function handleFootnote(phrase: string, range: Range) {
    const chapterId = selectedId();
    const article = articleRef();
    if (!chapterId || !article) return;

    const occurrence = occurrenceIndex(range, article, phrase);
    if (occurrence < 0) {
      setError(`Could not locate "${phrase}" in the chapter source.`);
      return;
    }

    const context = paragraphContext(range, article);

    try {
      const result = await invoke<ChapterContent>("add_footnote", {
        chapterId,
        selection: phrase,
        occurrenceIndex: occurrence,
        context,
      });
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content, result.notes) }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleEndnote(phrase: string, range: Range) {
    const chapterId = selectedId();
    const article = articleRef();
    if (!chapterId || !article) return;

    const occurrence = occurrenceIndex(range, article, phrase);
    if (occurrence < 0) {
      setError(`Could not locate "${phrase}" in the chapter source.`);
      return;
    }

    const context = paragraphContext(range, article);

    try {
      const result = await invoke<ChapterContent>("add_endnote", {
        chapterId,
        selection: phrase,
        occurrenceIndex: occurrence,
        context,
      });
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content, result.notes) }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDeleteNote(noteId: number) {
    const chapterId = selectedId();
    if (!chapterId) return;
    try {
      const result = await invoke<ChapterContent>("delete_note", {
        chapterId,
        noteId,
      });
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content, result.notes) }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDefine(word: string, range: Range) {
    const chapterId = selectedId();
    const article = articleRef();
    if (!chapterId || !article) return;

    const occurrence = occurrenceIndex(range, article, word);
    if (occurrence < 0) {
      setError(`Could not locate "${word}" in the chapter source.`);
      return;
    }

    const context = paragraphContext(range, article);

    try {
      const result = await invoke<ChapterContent>("define_word", {
        chapterId,
        word,
        occurrenceIndex: occurrence,
        context,
      });
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content, result.notes) }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div class="book-shell">
      <aside class="book-sidebar">
        <button class="book-title" onClick={() => setSelectedId(null)}>
          {manifest().metadata.title}
        </button>
        <nav class="chapter-list" aria-label="Chapters">
          <ol>
            <For each={manifest().lessonPlan.chapters}>
              {(ch) => (
                <li>
                  <button
                    class={`chapter-item chapter-item--${ch.status}${selectedId() === ch.id ? " chapter-item--active" : ""}`}
                    onClick={() => selectChapter(ch)}
                    disabled={ch.status === "generating"}
                  >
                    <span class="chapter-status-dot" />
                    {ch.title}
                  </button>
                </li>
              )}
            </For>
          </ol>
        </nav>
      </aside>

      <main class="book-content">
        {error() && <p class="field-error book-error" role="alert">{error()}</p>}

        <Switch>
          <Match when={!selectedChapter()}>
            <BookOverview manifest={manifest()} />
          </Match>

          <Match when={selectedChapter()?.status === "planned"}>
            <ChapterPlaceholder
              chapter={selectedChapter()!}
              onGenerate={() => generateChapter(selectedChapter()!.id)}
              generating={generating()}
              locked={selectedChapter()!.id !== nextPlannedId()}
            />
          </Match>

          <Match when={selectedChapter()?.status === "generating"}>
            <div class="chapter-generating">
              <div class="generating-spinner" />
              <p>Writing "{selectedChapter()!.title}"…</p>
            </div>
          </Match>

          <Match when={selectedChapter()?.status === "generated"}>
            <Show when={currentHtml()} fallback={<div class="chapter-loading">Loading…</div>}>
              <div class="reader">
                <div class="reader-spread">
                  <div class="page-window">
                    <article
                      ref={setArticleRef}
                      class="chapter-content prose"
                      innerHTML={currentHtml()!}
                    />
                  </div>
                  <aside class="margin-column" aria-label="Margin notes">
                    <For each={visibleMarginalia()}>
                      {(m) => (
                        <div class="margin-item">
                          <button
                            type="button"
                            class="margin-delete"
                            aria-label={`Remove note for ${m.word}`}
                            onClick={() => handleDeleteNote(m.id)}
                          >
                            ×
                          </button>
                          <div class="margin-word">{m.word}</div>
                          <div class="margin-body">{m.body}</div>
                        </div>
                      )}
                    </For>
                  </aside>
                </div>
                <nav class="reader-controls" aria-label="Pagination">
                  <button
                    type="button"
                    class="reader-nav-btn"
                    onClick={prev}
                    disabled={currentPage() === 0}
                  >
                    Previous
                  </button>
                  <span class="page-indicator" aria-live="polite">
                    Page {currentPage() + 1} of {pageCount()}
                  </span>
                  <button
                    type="button"
                    class="reader-nav-btn"
                    onClick={next}
                    disabled={currentPage() >= pageCount() - 1}
                  >
                    Next
                  </button>
                  <Show when={endnoteReturnPage() !== null}>
                    <button
                      type="button"
                      class="reader-back-btn"
                      onClick={returnFromEndnote}
                      aria-label={`Back to page ${endnoteReturnPage()! + 1}`}
                    >
                      ← Back to page {endnoteReturnPage()! + 1}
                    </button>
                  </Show>
                </nav>
              </div>
            </Show>
          </Match>
        </Switch>

        <SelectionToolbar
          container={articleRef}
          onDefine={handleDefine}
          onFootnote={handleFootnote}
          onEndnote={handleEndnote}
        />

        <Show when={activeFootnote()}>
          <div
            class="footnote-sheet-backdrop"
            onClick={() => setActiveFootnote(null)}
            role="presentation"
          >
            <div
              class="footnote-sheet"
              role="dialog"
              aria-label="Footnote"
              onClick={(e) => e.stopPropagation()}
            >
              <div class="footnote-sheet-header">
                <span class="footnote-sheet-title">Footnote {activeFootnote()!.id}</span>
                <button
                  type="button"
                  class="footnote-sheet-close"
                  aria-label="Close footnote"
                  onClick={() => setActiveFootnote(null)}
                >
                  ×
                </button>
              </div>
              <div class="footnote-sheet-body">{activeFootnote()!.body}</div>
            </div>
          </div>
        </Show>
      </main>
    </div>
  );
}

function BookOverview(props: { manifest: Manifest }) {
  return (
    <div class="book-overview">
      <h1 class="overview-title">{props.manifest.metadata.title}</h1>
      <Show when={props.manifest.metadata.subtitle}>
        <p class="overview-subtitle">{props.manifest.metadata.subtitle}</p>
      </Show>
      <Show when={props.manifest.metadata.description}>
        <p class="overview-description">{props.manifest.metadata.description}</p>
      </Show>
    </div>
  );
}

function ChapterPlaceholder(props: {
  chapter: Chapter;
  onGenerate: () => void;
  generating: boolean;
  locked: boolean;
}) {
  return (
    <div class="chapter-placeholder">
      <h2>{props.chapter.title}</h2>
      <p class="placeholder-description">{props.chapter.description}</p>
      <Show
        when={!props.locked}
        fallback={
          <p class="chapter-locked-message">
            Generate the previous chapters before accessing this one.
          </p>
        }
      >
        <button class="btn-primary" disabled={props.generating} onClick={props.onGenerate}>
          {props.generating ? "Generating…" : "Generate Chapter"}
        </button>
      </Show>
    </div>
  );
}
