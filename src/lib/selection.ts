const WORD_CHAR = /[\p{L}\p{N}'\-]/u;

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
