const WORD_CHAR = /[\p{L}\p{N}'\-]/u;

/**
 * Returns a new Range covering the full word containing the given range's
 * start position, expanded outward to word boundaries. Returns null when
 * the range crosses element boundaries or contains no word characters.
 */
export function expandRangeToWord(range: Range): Range | null {
  const node = range.startContainer;
  if (node.nodeType !== Node.TEXT_NODE || range.endContainer !== node)
    return null;

  const full = node.textContent ?? "";

  let anchor = range.startOffset;
  while (anchor < range.endOffset && !WORD_CHAR.test(full[anchor] ?? ""))
    anchor++;
  if (anchor >= full.length || !WORD_CHAR.test(full[anchor] ?? "")) return null;

  let start = anchor;
  let end = anchor;
  while (start > 0 && WORD_CHAR.test(full[start - 1] ?? "")) start--;
  while (end < full.length && WORD_CHAR.test(full[end] ?? "")) end++;

  const expanded = node.ownerDocument!.createRange();
  expanded.setStart(node, start);
  expanded.setEnd(node, end);
  return expanded;
}
