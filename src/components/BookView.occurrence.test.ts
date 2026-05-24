import { describe, it, expect, beforeEach } from "vitest";
import { occurrenceIndex, paragraphContext } from "./BookView";

function mountText(html: string): HTMLElement {
  document.body.innerHTML = "";
  const container = document.createElement("article");
  container.innerHTML = html;
  document.body.appendChild(container);
  return container;
}

function rangeAtTextOffset(container: HTMLElement, offset: number): Range {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  let remaining = offset;
  let node = walker.nextNode();
  while (node) {
    const len = (node.textContent ?? "").length;
    if (remaining <= len) {
      const r = document.createRange();
      r.setStart(node, remaining);
      r.setEnd(node, remaining);
      return r;
    }
    remaining -= len;
    node = walker.nextNode();
  }
  throw new Error("offset past end of container");
}

beforeEach(() => {
  document.body.innerHTML = "";
});

describe("occurrenceIndex", () => {
  it("returns 1 when the range starts on the first occurrence", () => {
    const c = mountText("<p>The blackhole is here.</p>");
    const range = rangeAtTextOffset(c, 4); // inside "blackhole"
    expect(occurrenceIndex(range, c, "blackhole")).toBe(1);
  });

  it("returns 2 when the range starts on the second occurrence", () => {
    const c = mountText("<p>First blackhole. Second blackhole here.</p>");
    const range = rangeAtTextOffset(c, "First blackhole. Second ".length + 2);
    expect(occurrenceIndex(range, c, "blackhole")).toBe(2);
  });

  it("skips matches that are not at word boundaries", () => {
    // "ole" inside "blackhole" should not count; only the standalone "ole" should.
    const c = mountText("<p>The blackhole has ole here.</p>");
    const range = rangeAtTextOffset(c, "The blackhole has ".length + 1);
    expect(occurrenceIndex(range, c, "ole")).toBe(1);
  });

  it("returns -1 when the word is not present at the range position", () => {
    const c = mountText("<p>no match here</p>");
    const range = rangeAtTextOffset(c, 0);
    expect(occurrenceIndex(range, c, "blackhole")).toBe(-1);
  });

  it("returns the containing paragraph as context", () => {
    const c = mountText("<p>Intro paragraph.</p><p>The blackhole is here.</p>");
    const range = rangeAtTextOffset(c, "Intro paragraph.".length + 4);
    expect(paragraphContext(range, c)).toBe("The blackhole is here.");
  });

  it("paragraphContext walks up to the closest block ancestor", () => {
    const c = mountText("<p>Text with <strong>bold word</strong> inside.</p>");
    const strong = c.querySelector("strong")!.firstChild!;
    const range = document.createRange();
    range.setStart(strong, 0);
    range.setEnd(strong, 0);
    expect(paragraphContext(range, c)).toBe("Text with bold word inside.");
  });

  it("paragraphContext truncates to the max length", () => {
    const long = "a ".repeat(800);
    const c = mountText(`<p>${long}</p>`);
    const range = rangeAtTextOffset(c, 0);
    expect(paragraphContext(range, c, 100).length).toBe(100);
  });

  it("finds a phrase that spans inline formatting", () => {
    const c = mountText("<p>The <strong>important fact</strong> matters.</p>");
    // Range starts at "The " text node, offset 4 (the space before strong).
    const firstText = c.querySelector("p")!.firstChild! as Text;
    const range = document.createRange();
    range.setStart(firstText, 4);
    range.setEnd(firstText, 4);
    expect(occurrenceIndex(range, c, "important fact")).toBe(1);
  });

  it("finds a phrase that starts in plain text and ends inside inline formatting", () => {
    const c = mountText("<p>The important <em>fact</em> matters.</p>");
    const firstText = c.querySelector("p")!.firstChild! as Text;
    const range = document.createRange();
    range.setStart(firstText, 4);
    range.setEnd(firstText, 4);
    expect(occurrenceIndex(range, c, "important fact")).toBe(1);
  });

  it("counts occurrences across element boundaries", () => {
    const c = mountText("<p>first blackhole</p><p>second blackhole</p>");
    // textContent is "first blacksecond blackhole" — wait, no, textContent
    // concatenates text nodes; with two <p>s the textContent has no separator,
    // so "first blackhole" + "second blackhole" = "first blackholesecond blackhole".
    // Verify the function still picks up the correct occurrence given the range.
    const target = c.querySelectorAll("p")[1].firstChild!;
    const range = document.createRange();
    range.setStart(target, "second ".length + 2);
    range.setEnd(target, "second ".length + 2);
    expect(occurrenceIndex(range, c, "blackhole")).toBe(2);
  });
});
