import { createSignal, onCleanup, onMount, Show } from "solid-js";
import { selectedSingleWord } from "../lib/selection";

interface Props {
  container: () => HTMLElement | undefined;
  onDefine?: (word: string, range: Range) => void;
}

export default function SelectionToolbar(props: Props) {
  const [word, setWord] = createSignal<string | null>(null);
  const [range, setRange] = createSignal<Range | null>(null);
  const [pos, setPos] = createSignal({ top: 0, left: 0 });

  function update() {
    const sel = document.getSelection();
    const containerEl = props.container();
    if (!sel || sel.rangeCount === 0 || !containerEl) {
      setWord(null);
      setRange(null);
      return;
    }
    const r = sel.getRangeAt(0);
    if (!containerEl.contains(r.commonAncestorContainer)) {
      setWord(null);
      setRange(null);
      return;
    }
    const w = selectedSingleWord(sel);
    if (!w) {
      setWord(null);
      setRange(null);
      return;
    }
    const rect = typeof r.getBoundingClientRect === "function"
      ? r.getBoundingClientRect()
      : null;
    if (rect) setPos({ top: rect.top, left: rect.left + rect.width / 2 });
    setRange(r.cloneRange());
    setWord(w);
  }

  function define() {
    const w = word();
    const r = range();
    if (!w || !r) return;
    props.onDefine?.(w, r);
  }

  onMount(() => {
    document.addEventListener("selectionchange", update);
    onCleanup(() => document.removeEventListener("selectionchange", update));
  });

  return (
    <Show when={word()}>
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
        <button type="button" class="selection-action" onClick={define}>
          Define
        </button>
      </div>
    </Show>
  );
}
