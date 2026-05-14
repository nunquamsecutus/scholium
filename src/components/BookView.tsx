import { For, Show } from "solid-js";
import type { Manifest } from "../types/manifest";

interface Props {
  manifest: Manifest;
}

export default function BookView(props: Props) {
  return (
    <div class="book-shell">
      <aside class="book-sidebar">
        <h2 class="book-title">{props.manifest.metadata.title}</h2>
        <nav class="chapter-list" aria-label="Chapters">
          <Show
            when={props.manifest.lessonPlan.chapters.length > 0}
            fallback={
              <p class="chapter-list-empty">
                Lesson plan not yet generated.
              </p>
            }
          >
            <ol>
              <For each={props.manifest.lessonPlan.chapters}>
                {(ch) => (
                  <li class={`chapter-item chapter-item--${ch.status}`}>
                    {ch.title}
                  </li>
                )}
              </For>
            </ol>
          </Show>
        </nav>
      </aside>

      <main class="book-content">
        <p class="book-content-placeholder">
          Lesson plan for "{props.manifest.metadata.topic}" will be generated here.
        </p>
      </main>
    </div>
  );
}
