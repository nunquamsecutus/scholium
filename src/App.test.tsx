import { describe, it, expect, vi } from "vitest";
import { render, fireEvent } from "@solidjs/testing-library";
import App from "./App";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
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

  it("Next button enables once text is entered", async () => {
    const { getByLabelText, getByRole } = render(() => <App />);
    const textarea = getByLabelText("What do you want to learn?");
    fireEvent.input(textarea, { target: { value: "How black holes form" } });
    expect(getByRole("button", { name: "Next" })).not.toBeDisabled();
  });

  it("renders the Open Existing Book option", () => {
    const { getByText } = render(() => <App />);
    expect(getByText("Open Existing Book")).toBeInTheDocument();
  });
});
