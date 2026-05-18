import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent } from "@solidjs/testing-library";
import SelectionToolbar from "./SelectionToolbar";

function mountArticle(text = "hello world"): HTMLElement {
  const article = document.createElement("article");
  article.textContent = text;
  document.body.appendChild(article);
  return article;
}

function selectInside(el: HTMLElement, start: number, end: number) {
  const textNode = el.firstChild as Text;
  const range = document.createRange();
  range.setStart(textNode, start);
  range.setEnd(textNode, end);
  const sel = window.getSelection()!;
  sel.removeAllRanges();
  sel.addRange(range);
  document.dispatchEvent(new Event("selectionchange"));
}

beforeEach(() => {
  window.getSelection()?.removeAllRanges();
  document.body.innerHTML = "";
});

describe("SelectionToolbar", () => {
  it("does not render without a selection", () => {
    const article = mountArticle();
    const { queryByRole } = render(() => <SelectionToolbar container={() => article} />);
    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("appears when a single word is selected inside the container", async () => {
    const article = mountArticle();
    const { findByRole } = render(() => <SelectionToolbar container={() => article} />);
    selectInside(article, 0, 5);
    expect(await findByRole("button", { name: "Define" })).toBeInTheDocument();
  });

  it("ignores selections outside the container", () => {
    const article = mountArticle();
    const outside = document.createElement("p");
    outside.textContent = "elsewhere";
    document.body.appendChild(outside);

    const { queryByRole } = render(() => <SelectionToolbar container={() => article} />);
    const range = document.createRange();
    range.setStart(outside.firstChild!, 0);
    range.setEnd(outside.firstChild!, 5);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));

    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("does not appear when no container is mounted", () => {
    mountArticle();
    const { queryByRole } = render(() => <SelectionToolbar container={() => undefined} />);
    selectInside(document.querySelector("article")!, 0, 5);
    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("does not appear for multi-word selections", () => {
    const article = mountArticle();
    const { queryByRole } = render(() => <SelectionToolbar container={() => article} />);
    selectInside(article, 0, 11);
    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("calls onDefine with the word and a Range when Define is clicked", async () => {
    const onDefine = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onDefine={onDefine} />
    ));
    selectInside(article, 0, 5);
    fireEvent.click(await findByRole("button", { name: "Define" }));

    expect(onDefine).toHaveBeenCalledTimes(1);
    const [word, range] = onDefine.mock.calls[0];
    expect(word).toBe("hello");
    expect(range).toBeInstanceOf(Range);
    expect(range.toString()).toBe("hello");
  });

  it("forwards a partial-word selection so the caller can expand it", async () => {
    const onDefine = vi.fn();
    const article = mountArticle("blackhole");
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onDefine={onDefine} />
    ));
    selectInside(article, 1, 4);
    fireEvent.click(await findByRole("button", { name: "Define" }));

    const [word, range] = onDefine.mock.calls[0];
    expect(word).toBe("blackhole");
    expect(range.toString()).toBe("lac");
  });
});
