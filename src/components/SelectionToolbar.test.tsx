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
    const { queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("appears when a single word is selected inside the container", async () => {
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 5);
    expect(await findByRole("button", { name: "Define" })).toBeInTheDocument();
  });

  it("ignores selections outside the container", () => {
    const article = mountArticle();
    const outside = document.createElement("p");
    outside.textContent = "elsewhere";
    document.body.appendChild(outside);

    const { queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
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
    const { queryByRole } = render(() => (
      <SelectionToolbar container={() => undefined} />
    ));
    selectInside(document.querySelector("article")!, 0, 5);
    expect(queryByRole("toolbar")).not.toBeInTheDocument();
  });

  it("shows Tell me more (not Define) for multi-word selections", async () => {
    const article = mountArticle();
    const { findByRole, queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 11);
    expect(
      await findByRole("button", { name: "Tell me more" }),
    ).toBeInTheDocument();
    expect(queryByRole("button", { name: "Define" })).not.toBeInTheDocument();
  });

  it("shows I don't understand alongside Tell me more for multi-word selections", async () => {
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 11);
    expect(
      await findByRole("button", { name: "I don't understand" }),
    ).toBeInTheDocument();
  });

  it("swaps to 'I still don't understand' when selection touches a rewrite span", async () => {
    const article = document.createElement("article");
    article.innerHTML = `<p>before <span data-rewrite-id="3">rewritten content</span> after</p>`;
    document.body.appendChild(article);
    const { findByRole, queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    const innerText = article.querySelector("span")!.firstChild as Text;
    const range = document.createRange();
    range.setStart(innerText, 0);
    range.setEnd(innerText, "rewritten content".length);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
    expect(
      await findByRole("button", { name: "I still don't understand" }),
    ).toBeInTheDocument();
    expect(
      queryByRole("button", { name: "I don't understand" }),
    ).not.toBeInTheDocument();
  });

  it("'I still don't understand' fires onRewriteConversation with the rewrite id", async () => {
    const onRewriteConversation = vi.fn();
    const article = document.createElement("article");
    article.innerHTML = `<p>before <span data-rewrite-id="7">rewritten content</span> after</p>`;
    document.body.appendChild(article);
    const { findByRole } = render(() => (
      <SelectionToolbar
        container={() => article}
        onRewriteConversation={onRewriteConversation}
      />
    ));
    const innerText = article.querySelector("span")!.firstChild as Text;
    const range = document.createRange();
    range.setStart(innerText, 0);
    range.setEnd(innerText, "rewritten content".length);
    const sel = window.getSelection()!;
    sel.removeAllRanges();
    sel.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
    fireEvent.click(
      await findByRole("button", { name: "I still don't understand" }),
    );

    expect(onRewriteConversation).toHaveBeenCalledTimes(1);
    const [rewriteId, r] = onRewriteConversation.mock.calls[0];
    expect(rewriteId).toBe("7");
    expect(r).toBeInstanceOf(Range);
  });

  it("Diagram fires onDrawDiagram with the phrase and range", async () => {
    const onDrawDiagram = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar
        container={() => article}
        onDrawDiagram={onDrawDiagram}
      />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Diagram" }));

    expect(onDrawDiagram).toHaveBeenCalledTimes(1);
    const [phrase, range] = onDrawDiagram.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("Draw a picture fires onDrawPicture with the phrase and range", async () => {
    const onDrawPicture = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar
        container={() => article}
        onDrawPicture={onDrawPicture}
      />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Draw a picture" }));

    expect(onDrawPicture).toHaveBeenCalledTimes(1);
    const [phrase, range] = onDrawPicture.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("I don't understand fires onRewrite with the phrase and range", async () => {
    const onRewrite = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onRewrite={onRewrite} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "I don't understand" }));

    expect(onRewrite).toHaveBeenCalledTimes(1);
    const [phrase, range] = onRewrite.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("Tell me more reveals the Footnote sub-action", async () => {
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Tell me more" }));
    expect(
      await findByRole("button", { name: "Footnote" }),
    ).toBeInTheDocument();
  });

  it("Footnote click fires onFootnote with the phrase and a range", async () => {
    const onFootnote = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onFootnote={onFootnote} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Tell me more" }));
    fireEvent.click(await findByRole("button", { name: "Footnote" }));

    expect(onFootnote).toHaveBeenCalledTimes(1);
    const [phrase, range] = onFootnote.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("Endnote click fires onEndnote with the phrase and a range", async () => {
    const onEndnote = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onEndnote={onEndnote} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Tell me more" }));
    fireEvent.click(await findByRole("button", { name: "Endnote" }));

    expect(onEndnote).toHaveBeenCalledTimes(1);
    const [phrase, range] = onEndnote.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("Appendix click fires onAppendix with the phrase and a range", async () => {
    const onAppendix = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onAppendix={onAppendix} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Tell me more" }));
    fireEvent.click(await findByRole("button", { name: "Appendix" }));

    expect(onAppendix).toHaveBeenCalledTimes(1);
    const [phrase, range] = onAppendix.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("Back returns from the expand sub-menu to the primary toolbar", async () => {
    const article = mountArticle();
    const { findByRole, queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Tell me more" }));
    fireEvent.click(await findByRole("button", { name: "Back" }));
    expect(
      await findByRole("button", { name: "Tell me more" }),
    ).toBeInTheDocument();
    expect(queryByRole("button", { name: "Footnote" })).not.toBeInTheDocument();
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

  it("shows Chat button for phrase selections when onChat is provided", async () => {
    const onChat = vi.fn();
    const article = mountArticle("hello world foo");
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onChat={onChat} />
    ));
    selectInside(article, 0, 11); // "hello world" — multi-word phrase
    expect(await findByRole("button", { name: "Chat" })).toBeInTheDocument();
  });

  it("does not show Chat button when onChat is not provided", async () => {
    const article = mountArticle("hello world foo");
    const { findByRole, queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 11);
    await findByRole("button", { name: "I don't understand" }); // wait for toolbar
    expect(queryByRole("button", { name: "Chat" })).not.toBeInTheDocument();
  });

  it("calls onChat with the phrase and Range when Chat is clicked", async () => {
    const onChat = vi.fn();
    const article = mountArticle("hello world foo");
    const { findByRole } = render(() => (
      <SelectionToolbar container={() => article} onChat={onChat} />
    ));
    selectInside(article, 0, 11);
    fireEvent.click(await findByRole("button", { name: "Chat" }));
    expect(onChat).toHaveBeenCalledTimes(1);
    const [phrase, range] = onChat.mock.calls[0];
    expect(phrase).toBe("hello world");
    expect(range).toBeInstanceOf(Range);
  });

  it("shows Read from here for single-word selection when onReadFromHere is provided", async () => {
    const onReadFromHere = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar
        container={() => article}
        onReadFromHere={onReadFromHere}
      />
    ));
    selectInside(article, 0, 5); // "hello" — single word
    expect(
      await findByRole("button", { name: "Read from here" }),
    ).toBeInTheDocument();
  });

  it("does not show Read from here when onReadFromHere is not provided", async () => {
    const article = mountArticle();
    const { findByRole, queryByRole } = render(() => (
      <SelectionToolbar container={() => article} />
    ));
    selectInside(article, 0, 5);
    await findByRole("button", { name: "Define" }); // wait for toolbar to appear
    expect(
      queryByRole("button", { name: "Read from here" }),
    ).not.toBeInTheDocument();
  });

  it("calls onReadFromHere with the word and Range when clicked", async () => {
    const onReadFromHere = vi.fn();
    const article = mountArticle();
    const { findByRole } = render(() => (
      <SelectionToolbar
        container={() => article}
        onReadFromHere={onReadFromHere}
      />
    ));
    selectInside(article, 0, 5);
    fireEvent.click(await findByRole("button", { name: "Read from here" }));

    expect(onReadFromHere).toHaveBeenCalledTimes(1);
    const [word, range] = onReadFromHere.mock.calls[0];
    expect(word).toBe("hello");
    expect(range).toBeInstanceOf(Range);
  });
});
