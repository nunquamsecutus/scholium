import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { selectedSingleWord, selectedPhrase } from "../lib/selection";

interface Props {
  container: () => HTMLElement | undefined;
  onDefine?: (word: string, range: Range) => void;
  onFootnote?: (phrase: string, range: Range) => void;
  onEndnote?: (phrase: string, range: Range) => void;
}

type Mode = "primary" | "expand";

export default function SelectionToolbar(props: Props) {
  const [word, setWord] = createSignal<string | null>(null);
  const [phrase, setPhrase] = createSignal<string | null>(null);
  const [range, setRange] = createSignal<Range | null>(null);
  const [pos, setPos] = createSignal({ top: 0, left: 0 });
  const [mode, setMode] = createSignal<Mode>("primary");

  function clear() {
    setWord(null);
    setPhrase(null);
    setRange(null);
    setMode("primary");
  }

  function update() {
    const sel = document.getSelection();
    const containerEl = props.container();
    if (!sel || sel.rangeCount === 0 || !containerEl) {
      clear();
      return;
    }
    const r = sel.getRangeAt(0);
    if (!containerEl.contains(r.commonAncestorContainer)) {
      clear();
      return;
    }
    const w = selectedSingleWord(sel);
    const p = w ? null : selectedPhrase(sel);
    if (!w && !p) {
      clear();
      return;
    }
    const rect = typeof r.getBoundingClientRect === "function"
      ? r.getBoundingClientRect()
      : null;
    if (rect) setPos({ top: rect.top, left: rect.left + rect.width / 2 });
    setRange(r.cloneRange());
    setWord(w);
    setPhrase(p);
    setMode("primary");
  }

  function define() {
    const w = word();
    const r = range();
    if (!w || !r) return;
    props.onDefine?.(w, r);
  }

  function footnote() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onFootnote?.(p, r);
  }

  function endnote() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onEndnote?.(p, r);
  }

  onMount(() => {
    document.addEventListener("selectionchange", update);
    onCleanup(() => document.removeEventListener("selectionchange", update));
  });

  return (
    <Show when={word() || phrase()}>
      <div
        class="selection-toolbar"
        role="toolbar"
        aria-label="Selection actions"
        style={{
          position: "fixed",
          top: `${pos().top}px`,
          left: `${pos().left}px`,
          transform: "translate(-50%, calc(-100% - 8px))",
        }}
        onMouseDown={(e) => e.preventDefault()}
      >
        <Show when={mode() === "primary"}>
          <Show when={word()}>
            <button type="button" class="selection-action" onClick={define}>
              Define
            </button>
          </Show>
          <Show when={phrase()}>
            <button
              type="button"
              class="selection-action"
              onClick={() => setMode("expand")}
            >
              Tell me more
            </button>
          </Show>
        </Show>
        <Show when={mode() === "expand"}>
          <button
            type="button"
            class="selection-action selection-action--back"
            aria-label="Back"
            onClick={() => setMode("primary")}
          >
            ←
          </button>
          <button type="button" class="selection-action" onClick={footnote}>
            Footnote
          </button>
          <button type="button" class="selection-action" onClick={endnote}>
            Endnote
          </button>
        </Show>
      </div>
    </Show>
  );
}
