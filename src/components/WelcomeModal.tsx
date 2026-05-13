import { createSignal } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";

interface Props {
  onNext: (topic: string) => void;
}

export default function WelcomeModal(props: Props) {
  const [topic, setTopic] = createSignal("");

  async function handleOpenBook() {
    await open({
      multiple: false,
      title: "Open Book",
    });
    // File loading not yet implemented — format TBD
  }

  return (
    <div class="modal-overlay">
      <div class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
        <h1 id="modal-title" class="modal-app-name">Edu Harness</h1>
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
          />
        </div>

        <div class="modal-actions">
          <button
            class="btn-primary"
            disabled={topic().trim().length === 0}
            onClick={() => props.onNext(topic().trim())}
          >
            Next
          </button>
          <button class="btn-text" onClick={handleOpenBook}>
            Open Existing Book
          </button>
        </div>
      </div>
    </div>
  );
}
