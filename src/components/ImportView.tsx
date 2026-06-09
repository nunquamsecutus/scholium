import { createSignal, For, Show } from "solid-js";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { Manifest } from "../types/manifest";
import { reorder } from "../lib/reorder";

interface Props {
  directory: string;
  files: string[];
  onCancel: () => void;
  onImported: (manifest: Manifest) => void;
}

function basename(path: string): string {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? "imported-book";
}

export default function ImportView(props: Props) {
  const [ordered, setOrdered] = createSignal<string[]>(props.files);
  const [dragIndex, setDragIndex] = createSignal<number | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  async function confirm() {
    setError("");
    const dest = await save({
      title: "Save imported book",
      defaultPath: `${basename(props.directory)}.scholium`,
      filters: [{ name: "Scholium Book", extensions: ["scholium"] }],
    });
    if (!dest) return;

    setBusy(true);
    try {
      const manifest = await invoke<Manifest>("import_book", {
        sourceDir: props.directory,
        destPath: dest,
        orderedFiles: ordered(),
      });
      props.onImported(manifest);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function onDragOver(e: DragEvent) {
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
  }

  function onDrop(i: number) {
    const from = dragIndex();
    setDragIndex(null);
    if (from === null) return;
    setOrdered(reorder(ordered(), from, i));
  }

  return (
    <div class="import-shell">
      <div class="import-card">
        <header class="import-header">
          <h1 class="import-title">Order the chapters</h1>
          <p class="import-instructions">
            Drag the files into the order you want them to appear in your book.
          </p>
        </header>

        <ol class="import-list" role="list">
          <For each={ordered()}>
            {(file, i) => (
              <li
                class={`import-item${dragIndex() === i() ? " import-item--dragging" : ""}`}
                draggable={!busy()}
                onDragStart={(e) => {
                  setDragIndex(i());
                  if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
                }}
                onDragOver={onDragOver}
                onDrop={() => onDrop(i())}
                onDragEnd={() => setDragIndex(null)}
              >
                <span class="import-grip" aria-hidden="true">
                  ⠿
                </span>
                <span class="import-position">{i() + 1}.</span>
                <span class="import-filename">{file}</span>
              </li>
            )}
          </For>
        </ol>

        <Show when={error()}>
          <p class="field-error" role="alert">
            {error()}
          </p>
        </Show>

        <footer class="import-footer">
          <button class="btn-text" onClick={props.onCancel} disabled={busy()}>
            Cancel
          </button>
          <button
            class="btn-primary"
            onClick={confirm}
            disabled={busy() || ordered().length === 0}
          >
            {busy() ? "Importing…" : "Confirm"}
          </button>
        </footer>
      </div>
    </div>
  );
}
