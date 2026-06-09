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

async function selectReadingLevel(container: ParentNode, level = "Adult") {
  const card = Array.from(
    container.querySelectorAll(".reading-level-card"),
  ).find((el) => el.textContent?.includes(level)) as HTMLElement;
  fireEvent.click(card);
  fireEvent.click(
    container.querySelector(
      ".reading-level-actions .btn-primary",
    ) as HTMLElement,
  );
}

describe("OnboardingView", () => {
  it("shows the topic in the header", () => {
    const { getByText } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(getByText("black holes")).toBeInTheDocument();
  });

  it("shows reading level selector before conversation", () => {
    const { getByText } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(getByText("How do you prefer to read?")).toBeInTheDocument();
    expect(getByText("Child")).toBeInTheDocument();
    expect(getByText("Teen")).toBeInTheDocument();
    expect(getByText("Adult")).toBeInTheDocument();
    expect(getByText("Academic")).toBeInTheDocument();
  });

  it("Continue is disabled until a level is selected", () => {
    const { getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    expect(getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("Continue enables after selecting a level", () => {
    const { container, getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    const card = container.querySelector(".reading-level-card") as HTMLElement;
    fireEvent.click(card);
    expect(getByRole("button", { name: "Continue" })).not.toBeDisabled();
  });

  it("calls begin_onboarding after level is selected and shows LLM response", async () => {
    mockInvoke.mockResolvedValue("What is your background in physics?");
    const { container, findByText } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    await selectReadingLevel(container);
    expect(
      await findByText("What is your background in physics?"),
    ).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("begin_onboarding", {
      topic: "black holes",
      readingLevel: "adult",
    });
  });

  it.each([
    ["Child", "child"],
    ["Teen", "teen"],
    ["Adult", "adult"],
    ["Academic", "academic"],
  ])(
    "selecting %s passes readingLevel '%s' to begin_onboarding",
    async (label, level) => {
      mockInvoke.mockResolvedValue("Hello!");
      const { container } = render(() => (
        <OnboardingView topic="test" onBook={noop} onBack={noop} />
      ));
      await selectReadingLevel(container, label);
      expect(mockInvoke).toHaveBeenCalledWith("begin_onboarding", {
        topic: "test",
        readingLevel: level,
      });
    },
  );

  it("selected card receives the --selected class", () => {
    const { container } = render(() => (
      <OnboardingView topic="test" onBook={noop} onBack={noop} />
    ));
    const teenCard = Array.from(
      container.querySelectorAll(".reading-level-card"),
    ).find((el) => el.textContent?.includes("Teen")) as HTMLElement;
    fireEvent.click(teenCard);
    expect(teenCard).toHaveClass("reading-level-card--selected");
  });

  it("previously selected card loses --selected when another is chosen", () => {
    const { container } = render(() => (
      <OnboardingView topic="test" onBook={noop} onBack={noop} />
    ));
    const cards = container.querySelectorAll(".reading-level-card");
    const [first, second] = [cards[0] as HTMLElement, cards[1] as HTMLElement];
    fireEvent.click(first);
    fireEvent.click(second);
    expect(first).not.toHaveClass("reading-level-card--selected");
    expect(second).toHaveClass("reading-level-card--selected");
  });

  it("passes readingLevel to generate_lesson_plan", async () => {
    mockInvoke
      .mockResolvedValueOnce("Tell me what you know.")
      .mockResolvedValue({
        summary: "s",
        description: "d",
        priorKnowledge: "p",
        lessonPlan: { summary: "s", chapters: [] },
      });

    const { container, findByRole, getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    await selectReadingLevel(container, "Teen");
    await findByRole("button", { name: /Generate Lesson Plan/ });
    fireEvent.click(getByRole("button", { name: /Generate Lesson Plan/ }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "generate_lesson_plan",
        expect.objectContaining({ topic: "black holes", readingLevel: "teen" }),
      );
    });
  });

  it("Generate Lesson Plan is disabled before first LLM response", async () => {
    mockInvoke.mockReturnValue(new Promise(() => {}));
    const { container, getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    await selectReadingLevel(container);
    expect(
      getByRole("button", { name: /Generate Lesson Plan/ }),
    ).toBeDisabled();
  });

  it("Generate Lesson Plan enables after first LLM response", async () => {
    mockInvoke.mockResolvedValue("Tell me what you know.");
    const { container, findByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    await selectReadingLevel(container);
    expect(
      await findByRole("button", { name: /Generate Lesson Plan/ }),
    ).not.toBeDisabled();
  });

  it("sends user message and calls continue_onboarding", async () => {
    mockInvoke
      .mockResolvedValueOnce("Tell me what you know.")
      .mockResolvedValueOnce("Great, one more question.");

    const { container, findByPlaceholderText, getByRole } = render(() => (
      <OnboardingView topic="black holes" onBook={noop} onBack={noop} />
    ));
    await selectReadingLevel(container);

    const input = await findByPlaceholderText("Type your response…");
    fireEvent.input(input, { target: { value: "I know some physics" } });
    fireEvent.click(getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "continue_onboarding",
        expect.objectContaining({
          topic: "black holes",
          readingLevel: "adult",
        }),
      );
    });
  });
});
