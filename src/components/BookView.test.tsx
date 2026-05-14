import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import BookView from "./BookView";
import type { Manifest } from "../types/manifest";

const emptyManifest: Manifest = {
  version: 1,
  metadata: {
    title: "Black Holes",
    topic: "How black holes form",
    prompt: "How black holes form",
    created: "2026-05-13T00:00:00Z",
    modified: "2026-05-13T00:00:00Z",
  },
  lessonPlan: {
    summary: "",
    chapters: [],
  },
};

const manifestWithChapters: Manifest = {
  ...emptyManifest,
  lessonPlan: {
    summary: "An overview.",
    chapters: [
      {
        id: "ch-01",
        title: "Stellar Evolution",
        description: "How stars live and die.",
        file: "chapters/01-stellar-evolution.md",
        status: "generated",
      },
      {
        id: "ch-02",
        title: "Gravitational Collapse",
        description: "The mechanics of collapse.",
        file: "chapters/02-gravitational-collapse.md",
        status: "planned",
      },
    ],
  },
};

describe("BookView", () => {
  it("renders the book title in the sidebar", () => {
    const { getByText } = render(() => <BookView manifest={emptyManifest} />);
    expect(getByText("Black Holes")).toBeInTheDocument();
  });

  it("shows placeholder when no chapters exist", () => {
    const { getByText } = render(() => <BookView manifest={emptyManifest} />);
    expect(getByText("Lesson plan not yet generated.")).toBeInTheDocument();
  });

  it("renders chapter titles when present", () => {
    const { getByText } = render(() => <BookView manifest={manifestWithChapters} />);
    expect(getByText("Stellar Evolution")).toBeInTheDocument();
    expect(getByText("Gravitational Collapse")).toBeInTheDocument();
  });

  it("applies correct status class to chapter items", () => {
    const { getByText } = render(() => <BookView manifest={manifestWithChapters} />);
    expect(getByText("Stellar Evolution").closest("li")).toHaveClass("chapter-item--generated");
    expect(getByText("Gravitational Collapse").closest("li")).toHaveClass("chapter-item--planned");
  });
});
