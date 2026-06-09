import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, waitFor } from "@solidjs/testing-library";
import SettingsModal from "./SettingsModal";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const baseSettings = {
  provider: "claude",
  claudeConfigured: true,
  ollamaUrl: "http://localhost:11434",
  ollamaModel: "llama3.2",
  imageQuality: "medium",
};

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(baseSettings);
    return Promise.resolve(baseSettings);
  });
});

describe("SettingsModal", () => {
  it("loads current settings into the form", async () => {
    const { getByText, getByRole } = render(() => (
      <SettingsModal onClose={() => {}} />
    ));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("get_settings"),
    );
    expect(getByText("Settings")).toBeInTheDocument();
    // The configured key surfaces the remove affordance.
    expect(
      getByRole("button", { name: "Remove stored key" }),
    ).toBeInTheDocument();
  });

  it("saves without a key change by sending null claudeApiKey", async () => {
    const onClose = vi.fn();
    const { getByRole } = render(() => <SettingsModal onClose={onClose} />);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("get_settings"),
    );

    fireEvent.click(getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({
          update: expect.objectContaining({
            claudeApiKey: null,
            imageQuality: "medium",
          }),
        }),
      );
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("sends the typed key on save", async () => {
    const { getByRole, getByPlaceholderText } = render(() => (
      <SettingsModal onClose={() => {}} />
    ));
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("get_settings"),
    );

    fireEvent.input(getByPlaceholderText(/leave blank to keep/), {
      target: { value: "sk-ant-new" },
    });
    fireEvent.click(getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({
          update: expect.objectContaining({ claudeApiKey: "sk-ant-new" }),
        }),
      );
    });
  });

  it("clears the key via the remove button", async () => {
    const { getByRole } = render(() => <SettingsModal onClose={() => {}} />);
    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith("get_settings"),
    );

    fireEvent.click(getByRole("button", { name: "Remove stored key" }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "update_settings",
        expect.objectContaining({
          update: expect.objectContaining({ clearClaudeKey: true }),
        }),
      );
    });
  });
});
