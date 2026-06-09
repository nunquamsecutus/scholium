import { describe, it, expect, vi } from "vitest";
import { render } from "@solidjs/testing-library";
import BookView from "./BookView";
import type { Manifest } from "../types/manifest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("marked", () => ({ marked: { parse: (s: string) => `<p>${s}</p>` } }));
vi.mock("dompurify", () => ({ default: { sanitize: (s: string) => s } }));

const baseManifest: Manifest = {
  version: 1,
  metadata: {
    title: "Black Holes",
    topic: "How black holes form",
    prompt: "How black holes form",
    created: "2026-05-14T00:00:00Z",
    modified: "2026-05-14T00:00:00Z",
    description: "A journey into the universe's most extreme objects.",
    readingLevel: "intermediate",
  },
  lessonPlan: {
    summary: "An overview of black holes.",
    chapters: [
      {
        id: "ch-01",
        title: "Stellar Evolution",
        file: "chapters/01-stellar-evolution.md",
        status: "planned",
      },
      {
        id: "ch-02",
        title: "Gravitational Collapse",
        file: "chapters/02-gravitational-collapse.md",
        status: "generated",
      },
    ],
  },
};

describe("BookView", () => {
  it("shows the book title in the sidebar", () => {
    const { getByRole } = render(() => <BookView manifest={baseManifest} />);
    expect(getByRole("navigation", { name: "Chapters" })).toBeInTheDocument();
    const sidebar = document.querySelector(".book-sidebar");
    expect(sidebar?.textContent).toContain("Black Holes");
  });

  it("clicking the book title returns to the overview", () => {
    const { getByText, getByRole } = render(() => (
      <BookView manifest={baseManifest} />
    ));
    // Navigate away from overview
    getByText("Stellar Evolution").closest("button")!.click();
    expect(
      getByRole("button", { name: "Generate Chapter" }),
    ).toBeInTheDocument();
    // Click title to go back
    (document.querySelector(".book-title") as HTMLElement).click();
    expect(
      getByText("A journey into the universe's most extreme objects."),
    ).toBeInTheDocument();
  });

  it("renders chapter titles in the sidebar", () => {
    const { getByText } = render(() => <BookView manifest={baseManifest} />);
    expect(getByText("Stellar Evolution")).toBeInTheDocument();
    expect(getByText("Gravitational Collapse")).toBeInTheDocument();
  });

  it("shows book overview with description before a chapter is selected", () => {
    const { getByText } = render(() => <BookView manifest={baseManifest} />);
    expect(
      getByText("A journey into the universe's most extreme objects."),
    ).toBeInTheDocument();
  });

  it("shows Generate Chapter button when the next planned chapter is selected", () => {
    const { getByText, getByRole } = render(() => (
      <BookView manifest={baseManifest} />
    ));
    getByText("Stellar Evolution").closest("button")!.click();
    expect(
      getByRole("button", { name: "Generate Chapter" }),
    ).toBeInTheDocument();
  });

  it("shows locked message when a planned chapter is selected but is not the next planned", () => {
    const allPlanned: Manifest = {
      ...baseManifest,
      lessonPlan: {
        ...baseManifest.lessonPlan,
        chapters: baseManifest.lessonPlan.chapters.map((ch) => ({
          ...ch,
          status: "planned" as const,
        })),
      },
    };
    const { getByText, queryByRole } = render(() => (
      <BookView manifest={allPlanned} />
    ));
    getByText("Gravitational Collapse").closest("button")!.click();
    expect(
      queryByRole("button", { name: "Generate Chapter" }),
    ).not.toBeInTheDocument();
    expect(getByText(/Generate the previous chapters/)).toBeInTheDocument();
  });

  it("applies status class to chapter items", () => {
    const { getByText } = render(() => <BookView manifest={baseManifest} />);
    expect(getByText("Stellar Evolution").closest("button")).toHaveClass(
      "chapter-item--planned",
    );
    expect(getByText("Gravitational Collapse").closest("button")).toHaveClass(
      "chapter-item--generated",
    );
  });
});
