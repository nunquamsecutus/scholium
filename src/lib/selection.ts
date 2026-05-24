/**
 * Selection utilities.
 *
 * # Source-position resolution
 *
 * The Rust renderer (`render.rs`) annotates every text span in the chapter HTML
 * with `data-src-start` and `data-src-end` UTF-8 byte offsets into the
 * reconstructed markdown source.  Elements whose visible content does NOT
 * correspond to source bytes (note anchors, artifact figures, appendix refs)
 * carry `data-src-skip`; the walker ignores their entire subtree.
 *
 * `resolveSourceRange` translates a browser `Range` into `{ srcStart, srcEnd }`
 * byte offsets that can be sent directly to the Tauri backend.  No text
 * searching or occurrence counting is performed — the mapping is exact.
 *
 * ## UTF-8 byte arithmetic
 *
 * `data-src-start` / `data-src-end` are UTF-8 byte positions (Rust's native
 * string indexing).  When the selection starts or ends mid-span, we compute the
 * byte length of the prefix using `TextEncoder`, which also produces UTF-8.
 * This ensures JS character offsets are correctly converted to byte offsets even
 * for multi-byte characters.
 */

const WORD_CHAR = /[\p{L}\p{N}'\-]/u;
const encoder = new TextEncoder();

/** Byte length of `s` encoded as UTF-8, matching Rust's str byte indexing. */
function utf8ByteLength(s: string): number {
  return encoder.encode(s).length;
}

/**
 * Walk up from `node` to find the nearest ancestor (or the node itself if it
 * is an Element) that has a `data-src-start` attribute, without crossing any
 * `data-src-skip` boundary.  Returns null if none is found or if the node
 * lives inside a skip subtree.
 */
function nearestSrcStart(node: Node, container: HTMLElement): HTMLElement | null {
  let n: Node | null = node.nodeType === Node.TEXT_NODE ? node.parentNode : node;
  while (n && n !== container) {
    const el = n as HTMLElement;
    if (el.hasAttribute?.("data-src-skip")) return null;
    if (el.hasAttribute?.("data-src-start")) return el;
    n = n.parentNode;
  }
  return null;
}

function nearestSrcEnd(node: Node, container: HTMLElement): HTMLElement | null {
  let n: Node | null = node.nodeType === Node.TEXT_NODE ? node.parentNode : node;
  while (n && n !== container) {
    const el = n as HTMLElement;
    if (el.hasAttribute?.("data-src-skip")) return null;
    if (el.hasAttribute?.("data-src-end")) return el;
    n = n.parentNode;
  }
  return null;
}

/**
 * Translate `range` into exact UTF-8 byte offsets in the reconstructed
 * markdown source.
 *
 * Returns `null` when:
 * - Either endpoint of the selection lands inside a `data-src-skip` subtree
 *   (e.g., a note-anchor superscript or an artifact figure).
 * - No annotated span ancestor is found (the selection is outside the
 *   rendered chapter content).
 */
export function resolveSourceRange(
  range: Range,
  container: HTMLElement,
): { srcStart: number; srcEnd: number } | null {
  // ── start position ────────────────────────────────────────────────────────
  const startSpan = nearestSrcStart(range.startContainer, container);
  if (!startSpan) return null;

  const spanSrcStart = Number(startSpan.dataset.srcStart);
  // Character offset within the span's text content up to the selection start.
  const startText = startSpan.textContent ?? "";
  // range.startContainer is a text node inside (or equal to) startSpan.
  // We need the character offset relative to the span's full text.
  let startCharOffset: number;
  if (range.startContainer.nodeType === Node.TEXT_NODE && startSpan.contains(range.startContainer)) {
    // Collect text content of startSpan up to range.startContainer.
    startCharOffset = textOffsetWithinSpan(startSpan, range.startContainer as Text, range.startOffset);
  } else {
    startCharOffset = 0;
  }
  const srcStart = spanSrcStart + utf8ByteLength(startText.slice(0, startCharOffset));

  // ── end position ─────────────────────────────────────────────────────────
  const endSpan = nearestSrcEnd(range.endContainer, container);
  if (!endSpan) return null;

  const spanSrcEnd = Number(endSpan.dataset.srcEnd);
  const endText = endSpan.textContent ?? "";
  let endCharOffset: number;
  if (range.endContainer.nodeType === Node.TEXT_NODE && endSpan.contains(range.endContainer)) {
    endCharOffset = textOffsetWithinSpan(endSpan, range.endContainer as Text, range.endOffset);
  } else {
    endCharOffset = endText.length;
  }
  // srcEnd = spanSrcEnd minus the bytes from endCharOffset to the end of the span.
  const srcEnd = spanSrcEnd - utf8ByteLength(endText.slice(endCharOffset));

  if (srcStart > srcEnd) return null;
  return { srcStart, srcEnd };
}

/**
 * Return the character offset of `charOffsetInNode` within `targetNode`
 * relative to the start of `span`'s text content, by walking text nodes in
 * document order.
 */
function textOffsetWithinSpan(span: HTMLElement, targetNode: Text, charOffsetInNode: number): number {
  const walker = document.createTreeWalker(span, NodeFilter.SHOW_TEXT);
  let offset = 0;
  let node = walker.nextNode() as Text | null;
  while (node) {
    if (node === targetNode) {
      return offset + charOffsetInNode;
    }
    offset += (node.textContent ?? "").length;
    node = walker.nextNode() as Text | null;
  }
  return offset;
}

/**
 * Returns true if the given Range intersects any element marked with
 * `[data-rewrite-id]` inside the container. Used to switch the toolbar from
 * "I don't understand" (first rewrite) to "I still don't understand" (open
 * the conversation dialog about an existing rewrite).
 */
export function selectionTouchesRewrite(range: Range, container: HTMLElement): boolean {
  const rewrites = container.querySelectorAll("[data-rewrite-id]");
  for (const r of Array.from(rewrites)) {
    if (range.intersectsNode(r)) return true;
  }
  return false;
}

/**
 * Returns the trimmed multi-word selection, or null if the selection is
 * empty, collapsed, or only one word. Used to distinguish single-word
 * actions (Define) from phrase-level actions (Tell me more).
 */
export function selectedPhrase(selection: Selection | null): string | null {
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;
  const trimmed = selection.toString().trim();
  if (!trimmed) return null;
  // Must contain internal whitespace to qualify as multi-word.
  if (!/\s/.test(trimmed)) return null;
  return trimmed;
}

/**
 * Returns the single word covered by the current selection, expanding
 * partial selections outward to word boundaries and trimming surrounding
 * whitespace. Returns null when the selection spans multiple words, is
 * collapsed, or contains no word characters.
 */
export function selectedSingleWord(selection: Selection | null): string | null {
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) return null;

  const trimmed = selection.toString().trim();
  if (!trimmed) return null;
  if (/\s/.test(trimmed)) return null;

  const range = selection.getRangeAt(0);
  const node = range.startContainer;

  if (node.nodeType !== Node.TEXT_NODE || range.endContainer !== node) {
    return stripBoundaryPunctuation(trimmed) || null;
  }

  const full = node.textContent ?? "";

  let anchor = range.startOffset;
  while (anchor < range.endOffset && !WORD_CHAR.test(full[anchor] ?? "")) anchor++;
  if (anchor >= full.length || !WORD_CHAR.test(full[anchor] ?? "")) return null;

  let start = anchor;
  let end = anchor;
  while (start > 0 && WORD_CHAR.test(full[start - 1] ?? "")) start--;
  while (end < full.length && WORD_CHAR.test(full[end] ?? "")) end++;

  const word = full.slice(start, end);
  return word || null;
}

function stripBoundaryPunctuation(s: string): string {
  return s.replace(/^[^\p{L}\p{N}'\-]+|[^\p{L}\p{N}'\-]+$/gu, "");
}
