import { createSignal, createEffect, createMemo, For, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Chapter, Manifest } from "../types/manifest";
import SelectionToolbar from "./SelectionToolbar";
import BookLoader from "./BookLoader";
import { resolveSourceRange } from "../lib/selection";
import { speakText, stopSpeaking } from "../utils/tts";

// Pagination unit comes from the article's clientWidth at runtime — see the
// page-window / chapter-content split in App.css. The browser fits one column
// to the available content area, and clientWidth gives us that width exactly,
// so scrollLeft and getBoundingClientRect math stay aligned without a brittle
// hardcoded constant.

interface GenerateResult {
  html: string;
  manifest: Manifest;
}

interface NoteFromBackend {
  id: number;
  type: string;
  word: string;
  ctime: string;
  body: string;
}

interface ArtifactFromBackend {
  id: number;
  mimeType: string;
  semanticType: string;
  ctime: string;
  caption: string | null;
  aspectRatio: number;
  source: string;
}

interface ChapterContent {
  /** Source-annotated HTML from the Rust renderer. Set directly as innerHTML. */
  html: string;
  notes: NoteFromBackend[];
  artifacts: ArtifactFromBackend[];
}

interface AppendixResult {
  html: string;
  notes: NoteFromBackend[];
  artifacts: ArtifactFromBackend[];
  manifest: Manifest;
}

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

interface RewriteDialogState {
  rewriteId: number;
  passage: string;
  context: string;
  messages: ChatMessage[];
  sending: boolean;
}

interface Props {
  manifest: Manifest;
  /** Called when the user selects text and clicks "Chat". */
  onOpenChat?: (phrase: string, context: string) => void;
}

const BLOCK_TAGS = new Set(["P", "LI", "BLOCKQUOTE", "H1", "H2", "H3", "H4", "H5", "H6"]);

// Returns the text of the closest block-level ancestor (paragraph, list item,
// heading, blockquote) of `range`'s start, truncated to `max` characters.
// Falls back to the container's text if no block ancestor is found. Used to
// give the LLM enough surrounding context to pick the right sense of a word.
export function paragraphContext(range: Range, container: HTMLElement, max = 1000): string {
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

export default function BookView(props: Props) {
  const [manifest, setManifest] = createSignal(props.manifest);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [contentCache, setContentCache] = createSignal<Record<string, string>>({});
  const [chapterNotes, setChapterNotes] = createSignal<Record<string, NoteFromBackend[]>>({});
  const [notePages, setNotePages] = createSignal<Record<number, number>>({});
  const [activeFootnote, setActiveFootnote] = createSignal<NoteFromBackend | null>(null);
  const [endnoteReturnPage, setEndnoteReturnPage] = createSignal<number | null>(null);
  const [rewriteDialog, setRewriteDialog] = createSignal<RewriteDialogState | null>(null);
  const [dialogInput, setDialogInput] = createSignal("");
  // Message shown in the floating busy pill while a highlight-driven LLM
  // action is in flight; null when idle.
  const [busy, setBusy] = createSignal<string | null>(null);
  // Live preview streamed from the backend during image work: the active
  // pipeline phase, the current candidate SVG, and which render pass produced
  // it. `phase` is one of "composition" | "rendering" | "critique".
  const [imageProgress, setImageProgress] = createSignal<{
    phase: string;
    pass: number;
    max: number;
    svg: string;
  } | null>(null);
  // The artifact whose controls overlay is currently shown (id + the figure's
  // viewport rect for positioning), and whether the delete confirm is open.
  const [activeArtifact, setActiveArtifact] = createSignal<{
    id: number;
    rect: { top: number; left: number; width: number; height: number };
  } | null>(null);
  const [confirmingArtifactDelete, setConfirmingArtifactDelete] = createSignal(false);
  // Edit-image modal: the artifact id + surrounding context, the instruction
  // text, and whether the regenerate request is in flight.
  const [editArtifact, setEditArtifact] = createSignal<{ id: number; context: string } | null>(null);
  const [editInstruction, setEditInstruction] = createSignal("");
  const [editSending, setEditSending] = createSignal(false);
  // Zoom lightbox: holds the extracted SVG markup and caption while the
  // full-screen overlay is open.
  const [zoomedArtifact, setZoomedArtifact] = createSignal<{
    svgHtml: string;
    caption: string | null;
  } | null>(null);
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

  // Apply the current page by scrolling the column container. Dismiss any
  // open artifact controls since their anchored position would be stale.
  createEffect(() => {
    const article = articleRef();
    const page = currentPage();
    if (article) article.scrollLeft = page * article.clientWidth;
    setActiveArtifact(null);
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
      if (zoomedArtifact()) {
        if (e.key === "Escape") {
          setZoomedArtifact(null);
          e.preventDefault();
        }
        return;
      }
      if (editArtifact()) {
        if (e.key === "Escape" && !editSending()) {
          setEditArtifact(null);
          e.preventDefault();
        }
        return;
      }
      if (rewriteDialog()) {
        if (e.key === "Escape" && !rewriteDialog()!.sending) {
          setRewriteDialog(null);
          e.preventDefault();
        }
        return;
      }
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

  // Live image-generation progress streamed from the backend.
  onMount(() => {
    const unlisten = listen<{ phase: string; pass: number; max: number; svg: string }>(
      "image-progress",
      (e) => setImageProgress(e.payload),
    );
    onCleanup(() => {
      unlisten.then((un) => un());
    });
  });

  // "Read Aloud" menu item (⌘⇧R) reads the current chapter from the beginning.
  onMount(() => {
    const unlisten = listen("start-reading", () => handleReadFromBeginning());
    onCleanup(() => unlisten.then((un) => un()));
  });

  // Stop TTS when BookView is unmounted (e.g. user returns to the welcome screen).
  onCleanup(() => void stopSpeaking());

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

      const figure = target?.closest(".artifact") as HTMLElement | null;
      if (figure) {
        e.preventDefault();
        const id = Number(figure.dataset.artifactId);
        const rect = figure.getBoundingClientRect();
        setActiveArtifact({
          id,
          rect: { top: rect.top, left: rect.left, width: rect.width, height: rect.height },
        });
        setConfirmingArtifactDelete(false);
        return;
      }
      // A click anywhere else in the article dismisses the artifact controls.
      setActiveArtifact(null);

      const appRef = target?.closest(".appendix-ref") as HTMLElement | null;
      if (appRef) {
        e.preventDefault();
        const seq = appRef.dataset.appendixSeq;
        if (seq) {
          const ch = manifest().lessonPlan.chapters.find((c) => c.id === `ap-${seq}`);
          if (ch) selectChapter(ch);
        }
        return;
      }

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

  async function handleDeleteArtifact(artifactId: number) {
    setActiveArtifact(null);
    await runContentCommand("Removing image…", "delete_artifact", {
      chapterId: selectedId(),
      artifactId,
    });
  }

  function openEditArtifact(artifactId: number) {
    const article = articleRef();
    const figure = article?.querySelector(
      `[data-artifact-id="${artifactId}"]`,
    ) as HTMLElement | null;
    // Context for the model: the paragraph the float sits in, or the block
    // image's preceding paragraph.
    const context =
      figure?.closest("p")?.textContent ??
      figure?.previousElementSibling?.textContent ??
      "";
    setActiveArtifact(null);
    setEditArtifact({ id: artifactId, context });
    setEditInstruction("");
  }

  function handleZoomArtifact(artifactId: number) {
    const article = articleRef();
    const figure = article?.querySelector(
      `[data-artifact-id="${artifactId}"]`,
    ) as HTMLElement | null;
    if (!figure) return;
    const svgEl = figure.querySelector("svg");
    if (!svgEl) return;
    const caption = figure.querySelector("figcaption")?.textContent ?? null;
    setActiveArtifact(null);
    setZoomedArtifact({ svgHtml: svgEl.outerHTML, caption });
  }

  async function applyRegenerate() {
    const edit = editArtifact();
    const chapterId = selectedId();
    const instruction = editInstruction().trim();
    if (!edit || !chapterId || !instruction || editSending()) return;
    setEditSending(true);
    setImageProgress(null);
    setBusy("Redrawing…");
    try {
      const result = await invoke<ChapterContent>("regenerate_artifact", {
        chapterId,
        artifactId: edit.id,
        instruction,
        context: edit.context,
      });
      setContentCache((c) => ({
        ...c,
        [chapterId]: result.html,
      }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
      setEditArtifact(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setEditSending(false);
      setBusy(null);
      setImageProgress(null);
    }
  }

  async function selectChapter(ch: Chapter) {
    void stopSpeaking();
    setError("");
    setSelectedId(ch.id);
    setCurrentPage(0);
    setEndnoteReturnPage(null);
    setActiveArtifact(null);

    if (ch.status === "generated" && !contentCache()[ch.id]) {
      try {
        const result = await invoke<ChapterContent>("read_chapter", { chapterId: ch.id });
        setContentCache((c) => ({ ...c, [ch.id]: result.html }));
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
      setContentCache((c) => ({ ...c, [chapterId]: result.html }));
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

  // Shared wrapper for highlight-driven commands that return a ChapterContent.
  // Manages the busy indicator and applies the returned html + notes.
  async function runContentCommand(
    busyMessage: string,
    command: string,
    args: Record<string, unknown>,
  ) {
    const chapterId = selectedId();
    if (!chapterId) return;
    setImageProgress(null);
    setBusy(busyMessage);
    try {
      const result = await invoke<ChapterContent>(command, args);
      setContentCache((c) => ({ ...c, [chapterId]: result.html }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
      setImageProgress(null);
    }
  }

  // Resolve the chapter and exact source byte range for a selection.
  // Returns null (after setting an error) if the selection can't be mapped to
  // source positions (e.g., the user selected inside a note anchor or figure).
  function resolvePhrase(selectionText: string, range: Range) {
    const chapterId = selectedId();
    const article = articleRef();
    if (!chapterId || !article) return null;
    const sourceRange = resolveSourceRange(range, article);
    if (!sourceRange) {
      setError(`Could not locate "${selectionText}" in the chapter source.`);
      return null;
    }
    return {
      chapterId,
      srcStart: sourceRange.srcStart,
      srcEnd: sourceRange.srcEnd,
      selectionText,
      context: paragraphContext(range, article),
    };
  }

  async function handleFootnote(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    await runContentCommand("Writing footnote…", "add_footnote", {
      chapterId: r.chapterId,
      selectionText: r.selectionText,
      srcEnd: r.srcEnd,
      context: r.context,
    });
  }

  async function handleEndnote(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    await runContentCommand("Writing endnote…", "add_endnote", {
      chapterId: r.chapterId,
      selectionText: r.selectionText,
      srcEnd: r.srcEnd,
      context: r.context,
    });
  }

  function handleRewriteConversation(rewriteId: number, range: Range) {
    const article = articleRef();
    if (!article) return;
    const span = article.querySelector(
      `[data-rewrite-id="${rewriteId}"]`,
    ) as HTMLElement | null;
    if (!span) return;
    const passage = span.textContent ?? "";
    const context = paragraphContext(range, article);
    setRewriteDialog({
      rewriteId,
      passage,
      context,
      messages: [],
      sending: false,
    });
    setDialogInput("");
  }

  async function sendDialogMessage() {
    const dialog = rewriteDialog();
    const text = dialogInput().trim();
    if (!dialog || !text || dialog.sending) return;

    const userMsg: ChatMessage = { role: "user", content: text };
    const nextMessages = [...dialog.messages, userMsg];
    setRewriteDialog({ ...dialog, messages: nextMessages, sending: true });
    setDialogInput("");

    try {
      const reply = await invoke<string>("converse_about_rewrite", {
        passage: dialog.passage,
        context: dialog.context,
        history: nextMessages,
      });
      setRewriteDialog((d) =>
        d
          ? {
              ...d,
              messages: [...d.messages, { role: "assistant", content: reply }],
              sending: false,
            }
          : null,
      );
    } catch (e) {
      setError(String(e));
      setRewriteDialog((d) => (d ? { ...d, sending: false } : null));
    }
  }

  async function applyUnderstandingRewrite() {
    const dialog = rewriteDialog();
    const chapterId = selectedId();
    if (!dialog || !chapterId || dialog.sending) return;
    setRewriteDialog({ ...dialog, sending: true });

    try {
      const result = await invoke<ChapterContent>("rewrite_with_conversation", {
        chapterId,
        rewriteId: dialog.rewriteId,
        passage: dialog.passage,
        context: dialog.context,
        history: dialog.messages,
      });
      setContentCache((c) => ({ ...c, [chapterId]: result.html }));
      setChapterNotes((m) => ({ ...m, [chapterId]: result.notes }));
      setRewriteDialog(null);
    } catch (e) {
      setError(String(e));
      setRewriteDialog((d) => (d ? { ...d, sending: false } : null));
    }
  }

  function handleChat(phrase: string, range: Range) {
    const container = articleRef();
    if (!container) return;
    const context = paragraphContext(range, container);
    props.onOpenChat?.(phrase, context);
  }

  function handleReadFromBeginning() {
    const container = articleRef();
    if (!container) return;
    void speakText(container.textContent ?? "");
  }

  function handleReadFromHere(_word: string, range: Range) {
    const container = articleRef();
    if (!container) return;
    // Build a range from the selection start to the end of the article so the
    // user hears everything from the tapped word onwards.
    const readRange = document.createRange();
    readRange.selectNodeContents(container);
    readRange.setStart(range.startContainer, range.startOffset);
    void speakText(readRange.toString());
  }

  async function handleDrawPicture(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    await runContentCommand("Drawing a picture…", "add_image", {
      chapterId: r.chapterId,
      selectionText: r.selectionText,
      srcStart: r.srcStart,
      srcEnd: r.srcEnd,
      context: r.context,
    });
  }

  async function handleDrawDiagram(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    await runContentCommand("Drawing a diagram…", "add_diagram", {
      chapterId: r.chapterId,
      selectionText: r.selectionText,
      srcStart: r.srcStart,
      srcEnd: r.srcEnd,
      context: r.context,
    });
  }

  async function handleRewrite(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    await runContentCommand("Rewriting…", "rewrite_passage", {
      chapterId: r.chapterId,
      selectionText: r.selectionText,
      srcStart: r.srcStart,
      srcEnd: r.srcEnd,
      context: r.context,
    });
  }

  async function handleAppendix(phrase: string, range: Range) {
    const r = resolvePhrase(phrase, range);
    if (!r) return;
    setBusy("Writing appendix…");
    try {
      const result = await invoke<AppendixResult>("add_appendix", {
        chapterId: r.chapterId,
        selectionText: r.selectionText,
        srcEnd: r.srcEnd,
        context: r.context,
      });
      setManifest(result.manifest);
      setContentCache((c) => ({ ...c, [r.chapterId]: result.html }));
      setChapterNotes((m) => ({ ...m, [r.chapterId]: result.notes }));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function handleDeleteNote(noteId: number) {
    await runContentCommand("Removing note…", "delete_note", {
      chapterId: selectedId(),
      noteId,
    });
  }

  async function handleDefine(word: string, range: Range) {
    const r = resolvePhrase(word, range);
    if (!r) return;
    await runContentCommand("Looking up definition…", "define_word", {
      chapterId: r.chapterId,
      word: r.selectionText,
      srcEnd: r.srcEnd,
      context: r.context,
    });
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
          onReadFromHere={handleReadFromHere}
          onFootnote={handleFootnote}
          onEndnote={handleEndnote}
          onAppendix={handleAppendix}
          onRewrite={handleRewrite}
          onRewriteConversation={handleRewriteConversation}
          onDrawPicture={handleDrawPicture}
          onDrawDiagram={handleDrawDiagram}
          onChat={handleChat}
        />

        <Show when={busy()}>
          <div
            class={`busy-pill${imageProgress()?.svg ? " busy-pill--preview" : ""}`}
            role="status"
            aria-live="polite"
          >
            <Show when={imageProgress()?.svg}>
              {(svg) => (
                <div class="busy-preview" innerHTML={svg()} />
              )}
            </Show>
            <div class="busy-pill-row">
              <BookLoader />
              <span>
                {busy()}
                <Show when={imageProgress()}>
                  {(p) => {
                    if (p().phase === "composition") return <> (Composing…)</>;
                    if (p().phase === "critique") return <> (Critiquing…)</>;
                    return <> (Pass {p().pass}/{p().max})</>;
                  }}
                </Show>
              </span>
            </div>
          </div>
        </Show>

        <Show when={activeArtifact()}>
          {(a) => (
            <div
              class="artifact-controls"
              style={{
                top: `${a().rect.top + 6}px`,
                left: `${a().rect.left + a().rect.width - 6}px`,
                transform: "translateX(-100%)",
              }}
            >
              <Show
                when={confirmingArtifactDelete()}
                fallback={
                  <>
                    <button
                      type="button"
                      class="artifact-ctrl"
                      aria-label="Zoom in"
                      onClick={() => handleZoomArtifact(a().id)}
                    >
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <circle cx="11" cy="11" r="8"/>
                        <path d="m21 21-4.35-4.35"/>
                        <line x1="11" y1="8" x2="11" y2="14"/>
                        <line x1="8" y1="11" x2="14" y2="11"/>
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="artifact-ctrl"
                      aria-label="Edit image"
                      onClick={() => openEditArtifact(a().id)}
                    >
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M4 20h4L18.5 9.5l-4-4L4 16z" />
                        <path d="M13.5 6.5l4 4" />
                      </svg>
                    </button>
                    <button
                      type="button"
                      class="artifact-ctrl artifact-ctrl--danger"
                      aria-label="Delete image"
                      onClick={() => setConfirmingArtifactDelete(true)}
                    >
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M3 6h18" />
                        <path d="M8 6V4h8v2" />
                        <path d="M6 6l1 14h10l1-14" />
                        <path d="M10 11v6M14 11v6" />
                      </svg>
                    </button>
                  </>
                }
              >
                <span class="artifact-confirm-label">Delete image?</span>
                <button
                  type="button"
                  class="artifact-ctrl artifact-ctrl--danger"
                  onClick={() => handleDeleteArtifact(a().id)}
                >
                  Delete
                </button>
                <button
                  type="button"
                  class="artifact-ctrl"
                  onClick={() => setConfirmingArtifactDelete(false)}
                >
                  Cancel
                </button>
              </Show>
            </div>
          )}
        </Show>

        <Show when={editArtifact()}>
          <div
            class="rewrite-dialog-backdrop"
            role="presentation"
            onClick={() => {
              if (!editSending()) setEditArtifact(null);
            }}
          >
            <div
              class="rewrite-dialog"
              role="dialog"
              aria-label="Edit image"
              onClick={(e) => e.stopPropagation()}
            >
              <div class="rewrite-dialog-header">
                <span class="rewrite-dialog-title">Edit image</span>
                <button
                  type="button"
                  class="rewrite-dialog-close"
                  aria-label="Close"
                  disabled={editSending()}
                  onClick={() => setEditArtifact(null)}
                >
                  ×
                </button>
              </div>
              <form
                class="rewrite-dialog-input"
                onSubmit={(e) => {
                  e.preventDefault();
                  applyRegenerate();
                }}
              >
                <textarea
                  value={editInstruction()}
                  onInput={(e) => setEditInstruction(e.currentTarget.value)}
                  placeholder="Describe the change you want…"
                  rows={3}
                  disabled={editSending()}
                />
              </form>
              <div class="rewrite-dialog-footer">
                <button
                  type="button"
                  class="rewrite-dialog-understand"
                  disabled={!editInstruction().trim() || editSending()}
                  onClick={applyRegenerate}
                >
                  Regenerate
                </button>
              </div>
            </div>
          </div>
        </Show>

        <Show when={zoomedArtifact()}>
          {(z) => (
            <div
              class="artifact-zoom-backdrop"
              role="presentation"
              onClick={() => setZoomedArtifact(null)}
            >
              <div
                class="artifact-zoom-dialog"
                role="dialog"
                aria-label="Zoomed diagram"
                onClick={(e) => e.stopPropagation()}
              >
                <button
                  type="button"
                  class="artifact-zoom-close"
                  aria-label="Close zoom"
                  onClick={() => setZoomedArtifact(null)}
                >
                  ×
                </button>
                <div class="artifact-zoom-svg" innerHTML={z().svgHtml} />
                <Show when={z().caption}>
                  <figcaption class="artifact-zoom-caption">{z().caption}</figcaption>
                </Show>
              </div>
            </div>
          )}
        </Show>

        <Show when={rewriteDialog()}>
          <div
            class="rewrite-dialog-backdrop"
            role="presentation"
            onClick={() => {
              if (!rewriteDialog()!.sending) setRewriteDialog(null);
            }}
          >
            <div
              class="rewrite-dialog"
              role="dialog"
              aria-label="I still don't understand"
              onClick={(e) => e.stopPropagation()}
            >
              <div class="rewrite-dialog-header">
                <span class="rewrite-dialog-title">I still don't understand</span>
                <button
                  type="button"
                  class="rewrite-dialog-close"
                  aria-label="Close"
                  disabled={rewriteDialog()!.sending}
                  onClick={() => setRewriteDialog(null)}
                >
                  ×
                </button>
              </div>
              <div class="rewrite-dialog-passage">
                <div class="rewrite-dialog-passage-label">The passage</div>
                <div class="rewrite-dialog-passage-text">{rewriteDialog()!.passage}</div>
              </div>
              <div class="rewrite-dialog-conversation">
                <For each={rewriteDialog()!.messages}>
                  {(m) => (
                    <div class={`rewrite-dialog-message rewrite-dialog-message--${m.role}`}>
                      {m.content}
                    </div>
                  )}
                </For>
                <Show when={rewriteDialog()!.sending}>
                  <div class="rewrite-dialog-message rewrite-dialog-message--assistant rewrite-dialog-loading">
                    …
                  </div>
                </Show>
              </div>
              <form
                class="rewrite-dialog-input"
                onSubmit={(e) => {
                  e.preventDefault();
                  sendDialogMessage();
                }}
              >
                <textarea
                  value={dialogInput()}
                  onInput={(e) => setDialogInput(e.currentTarget.value)}
                  placeholder="What's confusing about this?"
                  rows={2}
                  disabled={rewriteDialog()!.sending}
                />
                <button
                  type="submit"
                  class="btn-primary"
                  disabled={!dialogInput().trim() || rewriteDialog()!.sending}
                >
                  Send
                </button>
              </form>
              <div class="rewrite-dialog-footer">
                <button
                  type="button"
                  class="rewrite-dialog-understand"
                  disabled={rewriteDialog()!.sending || rewriteDialog()!.messages.length === 0}
                  onClick={applyUnderstandingRewrite}
                >
                  I understand now
                </button>
              </div>
            </div>
          </div>
        </Show>

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
