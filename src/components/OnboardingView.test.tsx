import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, waitFor } from "@solidjs/testing-library";
import OnboardingView from "./OnboardingView";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: vi.fn().mockResolvedValue(null),
}));

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

beforeEach(() => {
  mockInvoke.mockReset();
});

const noop = () => {};

describe("OnboardingView", () => {
  it("shows the topic in the header", async () => {
    mockInvoke.mockResolvedValue("What do you already know about black holes?");
    const { getByText } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(getByText("black holes")).toBeInTheDocument();
  });

  it("calls begin_onboarding on mount and shows LLM response", async () => {
    mockInvoke.mockResolvedValue("What is your background in physics?");
    const { findByText } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(await findByText("What is your background in physics?")).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("begin_onboarding", { topic: "black holes" });
  });

  it("Generate Lesson Plan is disabled before first LLM response", () => {
    mockInvoke.mockReturnValue(new Promise(() => {})); // never resolves
    const { getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(getByRole("button", { name: /Generate Lesson Plan/ })).toBeDisabled();
  });

  it("Generate Lesson Plan enables after first LLM response", async () => {
    mockInvoke.mockResolvedValue("Tell me what you know.");
    const { findByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(await findByRole("button", { name: /Generate Lesson Plan/ })).not.toBeDisabled();
  });

  it("sends user message and calls continue_onboarding", async () => {
    mockInvoke
      .mockResolvedValueOnce("Tell me what you know.")   // begin_onboarding
      .mockResolvedValueOnce("Great, one more question."); // continue_onboarding

    const { findByPlaceholderText, getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));

    const input = await findByPlaceholderText("Type your response…");
    fireEvent.input(input, { target: { value: "I know some physics" } });
    fireEvent.click(getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "continue_onboarding",
        expect.objectContaining({ topic: "black holes" })
      );
    });
  });
});