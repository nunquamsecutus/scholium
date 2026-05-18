import { describe, it, expect, beforeEach } from "vitest";
import { selectedSingleWord } from "./selection";

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
