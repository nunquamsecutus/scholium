import { describe, it, expect, vi } from "vitest";
import { render } from "@solidjs/testing-library";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("./assets/logo.svg", () => ({ default: "" }));

describe("App", () => {
  it("renders the heading", () => {
    const { getByText } = render(() => <App />);
    expect(getByText("Welcome to Tauri + Solid")).toBeInTheDocument();
  });

  it("renders the greet button", () => {
    const { getByRole } = render(() => <App />);
    expect(getByRole("button", { name: "Greet" })).toBeInTheDocument();
  });
});
