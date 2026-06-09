/**
 * Tests for resolveSourceRange and paragraphContext.
 *
 * resolveSourceRange works against Rust-annotated HTML: text spans carry
 * data-src-start / data-src-end byte offsets; synthesized elements (note
 * anchors, artifact figures) carry data-src-skip.
 *
 * The UTF-8 byte arithmetic mirrors what the Rust renderer emits — for ASCII
 * content, character offsets and byte offsets are identical.
 */
import { describe, it, expect, beforeEach } from "vitest";
import { resolveSourceRange } from "../lib/selection";
import { paragraphContext } from "./BookView";

// ── Helpers ───────────────────────────────────────────────────────────────────

function mountHtml(html: string): HTMLElement {
  document.body.innerHTML = "";
  const container = document.createElement("article");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

/** Build a Range that selects exactly the text node content of `span`. */
function rangeForSpan(span: HTMLElement): Range {
  const textNode = span.firstChild as Text;
  const r = document.createRange();
  r.setStart(textNode, 0);
  r.setEnd(textNode, textNode.length);
  return r;
}

/** Build a Range that starts at `startOffset` in `startNode` and ends at
 *  `endOffset` in `endNode`. */
function rangeFromTo(
  startNode: Text,
  startOffset: number,
  endNode: Text,
  endOffset: number,
): Range {
  const r = document.createRange();
  r.setStart(startNode, startOffset);
  r.setEnd(endNode, endOffset);
  return r;
}

// ── resolveSourceRange ────────────────────────────────────────────────────────

describe("resolveSourceRange", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("returns src-start/end for a fully-selected annotated span", () => {
    // Simulate what Rust emits for a plain-text paragraph word.
    const c = mountHtml(
      `<p data-src-start="0" data-src-end="13">` +
        `<span data-src-start="0" data-src-end="13">Hello world.</span>` +
        `</p>`,
    );
    const span = c.querySelector("span")!;
    const range = rangeForSpan(span as HTMLElement);
    const result = resolveSourceRange(range, c);
    expect(result).toEqual({ srcStart: 0, srcEnd: 13 });
  });

  it("computes srcStart from a mid-span selection start", () => {
    // "Hello world." — selecting from char 6 ("world.")
    const c = mountHtml(
      `<p><span data-src-start="0" data-src-end="12">Hello world</span></p>`,
    );
    const span = c.querySelector("span")!;
    const textNode = span.firstChild as Text;
    // Start at offset 6 ("world"), end at 11 ("world")
    const range = rangeFromTo(textNode, 6, textNode, 11);
    const result = resolveSourceRange(range, c);
    // srcStart = 0 + 6 bytes = 6; srcEnd = 12 - 0 bytes trailing = 12 - 0 = 12...
    // "world" ends at char 11, trailing " " is 0 chars — wait:
    // span text is "Hello world" (11 chars). End at 11 = end of string.
    // srcEnd = spanSrcEnd - utf8ByteLen("") = 12 - 0 = 12
    expect(result?.srcStart).toBe(6);
    expect(result?.srcEnd).toBe(12);
  });

  it("computes srcEnd correctly when selection ends mid-span", () => {
    // span covers bytes 10–20 for text "Hello dear" (10 chars).
    // Select chars 0–5 = "Hello".
    const c = mountHtml(
      `<p><span data-src-start="10" data-src-end="20">Hello dear</span></p>`,
    );
    const span = c.querySelector("span")!;
    const textNode = span.firstChild as Text;
    const range = rangeFromTo(textNode, 0, textNode, 5);
    const result = resolveSourceRange(range, c);
    // srcStart = 10 + 0 = 10
    // srcEnd = 20 - utf8Len(" dear") = 20 - 5 = 15
    expect(result?.srcStart).toBe(10);
    expect(result?.srcEnd).toBe(15);
  });

  it("returns null when start is inside a data-src-skip element", () => {
    const c = mountHtml(
      `<p>` +
        `<sup data-src-skip data-note-id="1">📖</sup>` +
        `<span data-src-start="6" data-src-end="14"> is here</span>` +
        `</p>`,
    );
    const sup = c.querySelector("sup")!;
    const supText = sup.firstChild as Text;
    const span = c.querySelector("span")!;
    const spanText = span.firstChild as Text;
    // Start inside the sup (skip zone), end in the span
    const range = rangeFromTo(supText, 0, spanText, 3);
    const result = resolveSourceRange(range, c);
    expect(result).toBeNull();
  });

  it("returns null when end is inside a data-src-skip element", () => {
    const c = mountHtml(
      `<p>` +
        `<span data-src-start="0" data-src-end="5">Hello</span>` +
        `<sup data-src-skip data-note-id="1">📖</sup>` +
        `</p>`,
    );
    const span = c.querySelector("span")!;
    const spanText = span.firstChild as Text;
    const sup = c.querySelector("sup")!;
    const supText = sup.firstChild as Text;
    const range = rangeFromTo(spanText, 0, supText, 1);
    const result = resolveSourceRange(range, c);
    expect(result).toBeNull();
  });

  it("spans two adjacent annotated spans correctly", () => {
    // "The " at bytes 0–4, "world" at bytes 4–9 (after **).
    // The '**' markers occupy bytes 4-5 and 12-13 in source but are not in DOM.
    // For this test, simulate two adjacent spans.
    const c = mountHtml(
      `<p>` +
        `<span data-src-start="0" data-src-end="4">The </span>` +
        `<strong><span data-src-start="6" data-src-end="11">world</span></strong>` +
        `</p>`,
    );
    const spans = c.querySelectorAll("span");
    const firstText = spans[0].firstChild as Text;
    const secondText = spans[1].firstChild as Text;
    // Select from start of "The " to end of "world"
    const range = rangeFromTo(firstText, 0, secondText, 5);
    const result = resolveSourceRange(range, c);
    expect(result?.srcStart).toBe(0);
    expect(result?.srcEnd).toBe(11);
  });

  it("returns null when no annotated ancestor exists", () => {
    const c = mountHtml(`<p>plain text with no annotations</p>`);
    const textNode = c.querySelector("p")!.firstChild as Text;
    const range = rangeFromTo(textNode, 0, textNode, 5);
    const result = resolveSourceRange(range, c);
    expect(result).toBeNull();
  });
});

// ── paragraphContext ──────────────────────────────────────────────────────────

describe("paragraphContext", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("returns the containing paragraph text", () => {
    const c = mountHtml(
      `<p>Intro paragraph.</p>` +
        `<p><span data-src-start="17" data-src-end="41">The blackhole is here.</span></p>`,
    );
    const span = c.querySelectorAll("span")[0]!;
    const textNode = span.firstChild as Text;
    const range = document.createRange();
    range.setStart(textNode, 4);
    range.setEnd(textNode, 4);
    expect(paragraphContext(range, c)).toBe("The blackhole is here.");
  });

  it("walks up to the closest block ancestor through inline formatting", () => {
    const c = mountHtml(
      `<p><span data-src-start="0" data-src-end="30">Text with </span>` +
        `<strong><span data-src-start="10" data-src-end="19">bold word</span></strong>` +
        `<span data-src-start="21" data-src-end="30"> inside.</span></p>`,
    );
    const strong = c.querySelector("strong")!.querySelector("span")!;
    const textNode = strong.firstChild as Text;
    const range = document.createRange();
    range.setStart(textNode, 0);
    range.setEnd(textNode, 0);
    expect(paragraphContext(range, c)).toBe("Text with bold word inside.");
  });

  it("truncates to the max length", () => {
    const long = "a ".repeat(800);
    const c = mountHtml(`<p>${long}</p>`);
    const textNode = c.querySelector("p")!.firstChild as Text;
    const range = document.createRange();
    range.setStart(textNode, 0);
    range.setEnd(textNode, 0);
    expect(paragraphContext(range, c, 100).length).toBe(100);
  });
});
