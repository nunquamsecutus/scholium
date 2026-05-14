import { createSignal, For, Match, Show, Switch } from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import { marked } from "marked";
import DOMPurify from "dompurify";
import type { Chapter, Manifest } from "../types/manifest";

interface GenerateResult {
  content: string;
  manifest: Manifest;
}

interface Props {
  manifest: Manifest;
}

function renderMarkdown(md: string): string {
  return DOMPurify.sanitize(marked.parse(md) as string);
}

export default function BookView(props: Props) {
  const [manifest, setManifest] = createSignal(props.manifest);
  const [selectedId, setSelectedId] = createSignal<string | null>(null);
  const [contentCache, setContentCache] = createSignal<Record<string, string>>({});
  const [generating, setGenerating] = createSignal(false);
  const [error, setError] = createSignal("");

  const selectedChapter = () =>
    manifest().lessonPlan.chapters.find((ch) => ch.id === selectedId()) ?? null;

  const currentHtml = () =>
    selectedId() ? contentCache()[selectedId()!] ?? null : null;

  async function selectChapter(ch: Chapter) {
    setError("");
    setSelectedId(ch.id);

    if (ch.status === "generated" && !contentCache()[ch.id]) {
      try {
        const text = await invoke<string>("read_chapter", { chapterId: ch.id });
        setContentCache((c) => ({ ...c, [ch.id]: renderMarkdown(text) }));
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
          ch.id === chapterId ? { ...ch, status: "generating" as const } : ch
        ),
      },
    }));

    try {
      const result = await invoke<GenerateResult>("generate_chapter", { chapterId });
      setManifest(result.manifest);
      setContentCache((c) => ({ ...c, [chapterId]: renderMarkdown(result.content) }));
      setSelectedId(chapterId);
    } catch (e) {
      setError(String(e));
      setManifest((m) => ({
        ...m,
        lessonPlan: {
          ...m.lessonPlan,
          chapters: m.lessonPlan.chapters.map((ch) =>
            ch.id === chapterId ? { ...ch, status: "planned" as const } : ch
          ),
        },
      }));
    } finally {
      setGenerating(false);
    }
  }

  return (
    <div class="book-shell">
      <aside class="book-sidebar">
        <h2 class="book-title">{manifest().metadata.title}</h2>
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
            <BookOverview manifest={manifest()} onGenerate={generateChapter} generating={generating()} />
          </Match>

          <Match when={selectedChapter()?.status === "planned"}>
            <ChapterPlaceholder
              chapter={selectedChapter()!}
              onGenerate={() => generateChapter(selectedChapter()!.id)}
              generating={generating()}
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
              <article
                class="chapter-content prose"
                innerHTML={currentHtml()!}
              />
            </Show>
          </Match>
        </Switch>
      </main>
    </div>
  );
}

function BookOverview(props: {
  manifest: Manifest;
  onGenerate: (id: string) => void;
  generating: boolean;
}) {
  const firstPlanned = () =>
    props.manifest.lessonPlan.chapters.find((ch) => ch.status === "planned");

  return (
    <div class="book-overview">
      <h1 class="overview-title">{props.manifest.metadata.title}</h1>
      <Show when={props.manifest.metadata.subtitle}>
        <p class="overview-subtitle">{props.manifest.metadata.subtitle}</p>
      </Show>
      <Show when={props.manifest.metadata.description}>
        <p class="overview-description">{props.manifest.metadata.description}</p>
      </Show>
      <p class="overview-meta">
        {props.manifest.lessonPlan.chapters.length} chapters ·{" "}
        {props.manifest.metadata.readingLevel ?? "general"} level
      </p>
      <Show when={firstPlanned()}>
        <button
          class="btn-primary overview-start"
          disabled={props.generating}
          onClick={() => props.onGenerate(firstPlanned()!.id)}
        >
          {props.generating ? "Generating…" : `Start — Generate "${firstPlanned()!.title}"`}
        </button>
      </Show>
    </div>
  );
}

function ChapterPlaceholder(props: {
  chapter: Chapter;
  onGenerate: () => void;
  generating: boolean;
}) {
  return (
    <div class="chapter-placeholder">
      <h2>{props.chapter.title}</h2>
      <p class="placeholder-description">{props.chapter.description}</p>
      <button class="btn-primary" disabled={props.generating} onClick={props.onGenerate}>
        {props.generating ? "Generating…" : "Generate Chapter"}
      </button>
    </div>
  );
}
