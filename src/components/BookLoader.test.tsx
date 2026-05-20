import { describe, it, expect } from "vitest";
import { render } from "@solidjs/testing-library";
import BookLoader from "./BookLoader";

describe("BookLoader", () => {
  it("renders an svg with the book-loader class and a turning page", () => {
    const { container } = render(() => <BookLoader />);
    expect(container.querySelector("svg.book-loader")).toBeInTheDocument();
    expect(container.querySelector(".book-loader-page")).toBeInTheDocument();
  });

  it("merges an extra class when provided", () => {
    const { container } = render(() => <BookLoader class="extra" />);
    const svg = container.querySelector("svg.book-loader");
    expect(svg).toHaveClass("extra");
  });
});
