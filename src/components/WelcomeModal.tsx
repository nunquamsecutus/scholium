import { createSignal } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { Manifest } from "../types/manifest";

interface Props {
  onTopic: (topic: string) => void;
  onBook: (manifest: Manifest) => void;
  onImport: (directory: string, files: string[]) => void;
}

export default function WelcomeModal(props: Props) {
  const [topic, setTopic] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");

  async function handleImport() {
    setError("");
    const result = await open({
      title: "Choose a folder of markdown files",
      directory: true,
      multiple: false,
    });
    if (!result) return;
    const path = Array.isArray(result) ? result[0] : result;

    setBusy(true);
    try {
      const files = await invoke<string[]>("scan_markdown_directory", { path });
      if (files.length === 0) {
        setError("No markdown files found in that folder.");
        return;
      }
      props.onImport(path, files);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function handleOpenBook() {
    setError("");
    const result = await open({
      title: "Open Book",
      filters: [{ name: "Edu Book", extensions: ["edubook"] }],
      multiple: false,
    });
    if (!result) return;
    const path = Array.isArray(result) ? result[0] : result;

    setBusy(true);
    try {
      const manifest = await invoke<Manifest>("load_book", { path });
      props.onBook(manifest);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div class="modal-overlay">
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
        <h1 id="modal-title" class="modal-app-name">Scholium</h1>
        <p class="modal-tagline">Your personal interactive learning companion</p>

        <div class="modal-body">
          <label class="field-label" for="topic-input">
            What do you want to learn?
          </label>
          <textarea
            id="topic-input"
            class="topic-input"
            rows={4}
            placeholder="e.g. How black holes form, the history of the Roman Empire, how neural networks work…"
            value={topic()}
            onInput={(e) => setTopic(e.currentTarget.value)}
            disabled={busy()}
          />
          {error() && <p class="field-error" role="alert">{error()}</p>}
        </div>

        <div class="modal-actions">
          <button
            class="btn-primary"
            disabled={topic().trim().length === 0 || busy()}
            onClick={() => props.onTopic(topic().trim())}
          >
            Next
          </button>
          <button class="btn-text" onClick={handleOpenBook} disabled={busy()}>
            Open Existing Book
          </button>
          <button class="btn-text" onClick={handleImport} disabled={busy()}>
            Import Book
          </button>
        </div>
      </div>
    </div>
  );
}
