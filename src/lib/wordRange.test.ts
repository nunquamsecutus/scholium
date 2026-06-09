import { describe, it, expect, beforeEach } from "vitest";
import { expandRangeToWord } from "./wordRange";

function rangeIn(text: string, start: number, end: number): Range {
  document.body.innerHTML = "";
  const p = document.createElement("p");
  p.textContent = text;
  document.body.appendChild(p);
  const range = document.createRange();
  range.setStart(p.firstChild!, start);
  range.setEnd(p.firstChild!, end);
  return range;
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("expandRangeToWord", () => {
  it("returns the same word when the whole word is already selected", () => {
    expect(expandRangeToWord(rangeIn("hello world", 0, 5))?.toString()).toBe(
      "hello",
    );
  });

  it("expands a partial selection to the full word", () => {
    expect(expandRangeToWord(rangeIn("blackhole", 1, 4))?.toString()).toBe(
      "blackhole",
    );
  });

  it("returns null when the range has no word characters", () => {
    expect(expandRangeToWord(rangeIn("!!!", 0, 3))).toBeNull();
  });

  it("returns null when the range crosses element boundaries", () => {
    const p = document.createElement("p");
    p.innerHTML = "<strong>bo</strong>ld";
    document.body.appendChild(p);
    const range = document.createRange();
    range.setStart(p.querySelector("strong")!.firstChild!, 0);
    range.setEnd(p.lastChild!, 2);
    expect(expandRangeToWord(range)).toBeNull();
  });

  it("includes hyphens when expanding", () => {
    expect(
      expandRangeToWord(rangeIn("well-known author", 0, 4))?.toString(),
    ).toBe("well-known");
  });

  it("includes apostrophes when expanding", () => {
    expect(expandRangeToWord(rangeIn("don't worry", 1, 3))?.toString()).toBe(
      "don't",
    );
  });
});
