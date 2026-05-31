import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@solidjs/testing-library";
import ChatModal from "./ChatModal";

// Mock Tauri invoke
const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mockInvoke(...args) }));

const noopClose = vi.fn();
const noopChanged = vi.fn();

function renderModal() {
  return render(() => (
    <ChatModal onClose={noopClose} onManifestChanged={noopChanged} />
  ));
}

describe("ChatModal", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the header and empty state", () => {
    renderModal();
    expect(screen.getByRole("dialog", { name: "Chat with AI" })).toBeInTheDocument();
    expect(screen.getByText(/ask me anything/i)).toBeInTheDocument();
  });

  it("close button calls onClose", () => {
    renderModal();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(noopClose).toHaveBeenCalledOnce();
  });

  it("send button is disabled when input is empty", () => {
    renderModal();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("send button enables when input has text", async () => {
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "hello" } });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Send" })).not.toBeDisabled();
    });
  });

  it("sends a message and displays the assistant reply", async () => {
    mockInvoke.mockResolvedValue({ reply: "Hello from AI!", patch: null });
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "hi" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(screen.getByText("Hello from AI!")).toBeInTheDocument();
    });
    expect(mockInvoke).toHaveBeenCalledWith("chat_about_book", expect.objectContaining({
      history: expect.arrayContaining([
        expect.objectContaining({ role: "user", content: "hi" }),
      ]),
    }));
  });

  it("shows patch banner when reply contains a patch", async () => {
    mockInvoke.mockResolvedValue({
      reply: "I'll rename it.\n```json\n{\"action\":\"rename_chapter\",\"id\":\"ch-01\",\"title\":\"New Title\"}\n```",
      patch: { action: "rename_chapter", id: "ch-01", title: "New Title" },
    });
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "rename chapter 1" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(screen.getByText(/Rename chapter "ch-01" to "New Title"/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Apply" })).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Dismiss" })).toBeInTheDocument();
    });
  });

  it("applies patch and notifies onManifestChanged", async () => {
    const updatedManifest = {
      version: 1,
      metadata: { title: "Test", topic: "t", prompt: "p", created: "", modified: "" },
      lessonPlan: { summary: "s", chapters: [] },
    };
    // First call: chat reply with patch
    mockInvoke.mockResolvedValueOnce({
      reply: "Renaming it.",
      patch: { action: "rename_chapter", id: "ch-01", title: "New Title" },
    });
    // Second call: apply_manifest_patch
    mockInvoke.mockResolvedValueOnce(updatedManifest);

    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "rename" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => screen.getByRole("button", { name: "Apply" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("apply_manifest_patch", expect.objectContaining({
        patch: expect.objectContaining({ action: "rename_chapter" }),
      }));
      expect(noopChanged).toHaveBeenCalledWith(updatedManifest);
    });
  });

  it("dismissing patch shows acknowledgement in thread", async () => {
    mockInvoke.mockResolvedValueOnce({
      reply: "OK.",
      patch: { action: "remove_chapter", id: "ch-01" },
    });
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "remove chapter 1" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => screen.getByRole("button", { name: "Dismiss" }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));

    await waitFor(() => {
      expect(screen.getByText(/not applied/i)).toBeInTheDocument();
    });
  });

  it("shows error when invoke fails", async () => {
    mockInvoke.mockRejectedValue("network error");
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toHaveTextContent("network error");
    });
  });

  it("strips the json patch block from the displayed message", async () => {
    mockInvoke.mockResolvedValue({
      reply: "I suggest this change.\n```json\n{\"action\":\"rename_chapter\",\"id\":\"ch-01\",\"title\":\"New\"}\n```\nLet me know!",
      patch: { action: "rename_chapter", id: "ch-01", title: "New" },
    });
    renderModal();
    const input = screen.getByPlaceholderText(/message/i);
    fireEvent.input(input, { target: { value: "rename" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => {
      expect(screen.getByText(/i suggest this change/i)).toBeInTheDocument();
      // The raw ```json block should not appear in the thread
      expect(screen.queryByText(/```json/i)).not.toBeInTheDocument();
    });
  });
});
