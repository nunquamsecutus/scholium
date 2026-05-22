import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@solidjs/testing-library";
import App from "./App";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

describe("App / WelcomeModal", () => {
  it("renders the app name", () => {
    const { getByText } = render(() => <App />);
    expect(getByText("Edu Harness")).toBeInTheDocument();
  });

  it("renders the topic prompt label", () => {
    const { getByLabelText } = render(() => <App />);
    expect(getByLabelText("What do you want to learn?")).toBeInTheDocument();
  });

  it("Next button is disabled when textarea is empty", () => {
    const { getByRole } = render(() => <App />);
    expect(getByRole("button", { name: "Next" })).toBeDisabled();
  });

  it("Next button enables once text is entered", () => {
    const { getByLabelText, getByRole } = render(() => <App />);
    fireEvent.input(getByLabelText("What do you want to learn?"), {
      target: { value: "How black holes form" },
    });
    expect(getByRole("button", { name: "Next" })).not.toBeDisabled();
  });

  it("renders the Open Existing Book option", () => {
    const { getByText } = render(() => <App />);
    expect(getByText("Open Existing Book")).toBeInTheDocument();
  });
});
