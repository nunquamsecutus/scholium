import { describe, it, expect, beforeEach } from "vitest";
import { selectedSingleWord, selectedPhrase, selectionTouchesRewrite } from "./selection";

function selectRange(text: string, start: number, end: number): Selection {
  document.body.innerHTML = "";
  const p = document.createElement("p");
  p.textContent = text;
  document.body.appendChild(p);
  const textNode = p.firstChild as Text;
  const range = document.createRange();
  range.setStart(textNode, start);
  range.setEnd(textNode, end);
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  return sel;
}

describe("selectedSingleWord", () => {
  beforeEach(() => {
    window.getSelection()?.removeAllRanges();
    document.body.innerHTML = "";
  });

  it("returns null when given no selection", () => {
    expect(selectedSingleWord(null)).toBeNull();
  });

  it("returns the whole word when selected exactly", () => {
    expect(selectedSingleWord(selectRange("hello world", 0, 5))).toBe("hello");
  });

  it("expands a partial selection to the full word", () => {
    expect(selectedSingleWord(selectRange("blackhole", 1, 4))).toBe("blackhole");
  });

  it("trims surrounding whitespace from the selection", () => {
    expect(selectedSingleWord(selectRange("  hello  world", 0, 7))).toBe("hello");
  });

  it("returns null when the selection spans multiple words", () => {
    expect(selectedSingleWord(selectRange("hello world", 0, 11))).toBeNull();
  });

  it("returns null for a collapsed selection", () => {
    expect(selectedSingleWord(selectRange("hello", 2, 2))).toBeNull();
  });

  it("returns null when the selection contains only whitespace", () => {
    expect(selectedSingleWord(selectRange("  hello  ", 0, 2))).toBeNull();
  });

  it("returns null when the trimmed selection still has internal whitespace", () => {
    expect(selectedSingleWord(selectRange("hello world", 2, 9))).toBeNull();
  });

  it("includes hyphens in word boundaries", () => {
    expect(selectedSingleWord(selectRange("well-known author", 0, 4))).toBe("well-known");
  });

  it("includes apostrophes in word boundaries", () => {
    expect(selectedSingleWord(selectRange("don't worry", 1, 3))).toBe("don't");
  });

  it("handles a selection at the end of the text", () => {
    expect(selectedSingleWord(selectRange("hello world", 6, 11))).toBe("world");
  });

  it("strips trailing punctuation when the selection crosses element boundaries", () => {
    document.body.innerHTML = "";
    const p = document.createElement("p");
    p.innerHTML = "<strong>bo</strong>ld text";
    document.body.appendChild(p);
    const range = document.createRange();
    range.setStart(p.querySelector("strong")!.firstChild!, 0);
    range.setEnd(p.lastChild!, 2);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    expect(selectedSingleWord(sel)).toBe("bold");
  });
});

describe("selectedPhrase", () => {
  beforeEach(() => {
    window.getSelection()?.removeAllRanges();
    document.body.innerHTML = "";
  });

  it("returns null for a single word", () => {
    expect(selectedPhrase(selectRange("hello world", 0, 5))).toBeNull();
  });

  it("returns the trimmed phrase for a multi-word selection", () => {
    expect(selectedPhrase(selectRange("hello world here", 0, 11))).toBe("hello world");
  });

  it("returns null for null selection", () => {
    expect(selectedPhrase(null)).toBeNull();
  });

  it("returns null for a collapsed selection", () => {
    expect(selectedPhrase(selectRange("hello", 2, 2))).toBeNull();
  });

  it("returns null for whitespace-only selection", () => {
    expect(selectedPhrase(selectRange("hello   world", 5, 8))).toBeNull();
  });

  it("trims leading and trailing whitespace from the phrase", () => {
    expect(selectedPhrase(selectRange("  hello world  ", 0, 15))).toBe("hello world");
  });
});

describe("selectionTouchesRewrite", () => {
  beforeEach(() => {
    window.getSelection()?.removeAllRanges();
    document.body.innerHTML = "";
  });

  function rangeInside(node: Node, start: number, end: number): Range {
    const range = document.createRange();
    range.setStart(node, start);
    range.setEnd(node, end);
    return range;
  }

  it("returns false when no rewrite spans exist", () => {
    const article = document.createElement("article");
    article.textContent = "hello world";
    document.body.appendChild(article);
    const range = rangeInside(article.firstChild!, 0, 11);
    expect(selectionTouchesRewrite(range, article)).toBe(false);
  });

  it("returns true when selection is entirely inside a rewrite span", () => {
    const article = document.createElement("article");
    article.innerHTML = `<p>before <span data-rewrite-id="1">rewritten content</span> after</p>`;
    document.body.appendChild(article);
    const innerText = article.querySelector("span")!.firstChild!;
    const range = rangeInside(innerText, 0, "rewritten content".length);
    expect(selectionTouchesRewrite(range, article)).toBe(true);
  });

  it("returns true when selection crosses a rewrite span boundary", () => {
    const article = document.createElement("article");
    article.innerHTML = `<p>before <span data-rewrite-id="1">rewritten</span> after</p>`;
    document.body.appendChild(article);
    const p = article.querySelector("p")!;
    const range = document.createRange();
    range.setStart(p.firstChild!, 2);
    range.setEnd(p.lastChild!, 3);
    expect(selectionTouchesRewrite(range, article)).toBe(true);
  });

  it("returns false when selection is fully outside any rewrite span", () => {
    const article = document.createElement("article");
    article.innerHTML = `<p>before <span data-rewrite-id="1">rewritten</span> after</p>`;
    document.body.appendChild(article);
    const lastText = article.querySelector("p")!.lastChild!;
    const range = rangeInside(lastText, 1, 4);
    expect(selectionTouchesRewrite(range, article)).toBe(false);
  });
});
