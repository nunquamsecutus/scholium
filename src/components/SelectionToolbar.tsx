import { createSignal, onCleanup, onMount, Show } from "solid-js";
import {
  selectedSingleWord,
  selectedPhrase,
  selectionTouchesRewrite,
} from "../lib/selection";

interface Props {
  container: () => HTMLElement | undefined;
  onDefine?: (word: string, range: Range) => void;
  onReadFromHere?: (word: string, range: Range) => void;
  onFootnote?: (phrase: string, range: Range) => void;
  onEndnote?: (phrase: string, range: Range) => void;
  onAppendix?: (phrase: string, range: Range) => void;
  onRewrite?: (phrase: string, range: Range) => void;
  onRewriteConversation?: (rewriteId: number, range: Range) => void;
  onDrawPicture?: (phrase: string, range: Range) => void;
  onDrawDiagram?: (phrase: string, range: Range) => void;
  onChat?: (phrase: string, range: Range) => void;
}

type Mode = "primary" | "expand";

export default function SelectionToolbar(props: Props) {
  const [word, setWord] = createSignal<string | null>(null);
  const [phrase, setPhrase] = createSignal<string | null>(null);
  const [range, setRange] = createSignal<Range | null>(null);
  const [pos, setPos] = createSignal({ top: 0, left: 0 });
  const [mode, setMode] = createSignal<Mode>("primary");
  const [touchedRewriteId, setTouchedRewriteId] = createSignal<number | null>(
    null,
  );

  function clear() {
    setWord(null);
    setPhrase(null);
    setRange(null);
    setMode("primary");
    setTouchedRewriteId(null);
  }

  // Find the first rewrite span the range intersects, returning its id.
  function findTouchedRewriteId(
    r: Range,
    container: HTMLElement,
  ): number | null {
    const spans = container.querySelectorAll("[data-rewrite-id]");
    for (const span of Array.from(spans)) {
      if (r.intersectsNode(span)) {
        const id = Number((span as HTMLElement).dataset.rewriteId);
        return Number.isFinite(id) ? id : null;
      }
    }
    return null;
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
    const rect =
      typeof r.getBoundingClientRect === "function"
        ? r.getBoundingClientRect()
        : null;
    if (rect) setPos({ top: rect.top, left: rect.left + rect.width / 2 });
    setRange(r.cloneRange());
    setWord(w);
    setPhrase(p);
    setMode("primary");
    setTouchedRewriteId(
      p && selectionTouchesRewrite(r, containerEl)
        ? findTouchedRewriteId(r, containerEl)
        : null,
    );
  }

  function define() {
    const w = word();
    const r = range();
    if (!w || !r) return;
    props.onDefine?.(w, r);
  }

  function readFromHere() {
    const w = word();
    const r = range();
    if (!w || !r) return;
    props.onReadFromHere?.(w, r);
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

  function appendix() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onAppendix?.(p, r);
  }

  function rewrite() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    const id = touchedRewriteId();
    if (id !== null) {
      props.onRewriteConversation?.(id, r);
    } else {
      props.onRewrite?.(p, r);
    }
  }

  function drawPicture() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onDrawPicture?.(p, r);
  }

  function drawDiagram() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onDrawDiagram?.(p, r);
  }

  function chat() {
    const p = phrase();
    const r = range();
    if (!p || !r) return;
    props.onChat?.(p, r);
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
            <Show when={props.onReadFromHere}>
              <button
                type="button"
                class="selection-action"
                onClick={readFromHere}
              >
                Read from here
              </button>
            </Show>
          </Show>
          <Show when={phrase()}>
            <button
              type="button"
              class="selection-action"
              onClick={() => setMode("expand")}
            >
              Tell me more
            </button>
            <button type="button" class="selection-action" onClick={rewrite}>
              {touchedRewriteId() !== null
                ? "I still don't understand"
                : "I don't understand"}
            </button>
            <button
              type="button"
              class="selection-action"
              onClick={drawPicture}
            >
              Draw a picture
            </button>
            <button
              type="button"
              class="selection-action"
              onClick={drawDiagram}
            >
              Diagram
            </button>
            <Show when={props.onChat}>
              <button type="button" class="selection-action" onClick={chat}>
                Chat
              </button>
            </Show>
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
          <button type="button" class="selection-action" onClick={appendix}>
            Appendix
          </button>
        </Show>
      </div>
    </Show>
  );
}
