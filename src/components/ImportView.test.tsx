import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent, waitFor } from "@solidjs/testing-library";
import ImportView from "./ImportView";

const mockInvoke = vi.fn();
const mockSave = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: (...args: unknown[]) => mockSave(...args),
}));

beforeEach(() => {
  mockInvoke.mockReset();
  mockSave.mockReset();
});

describe("ImportView", () => {
  it("renders the file list in the provided order", () => {
    const { getByText } = render(() => (
      <ImportView
        directory="/tmp/src"
        files={["01-intro.md", "02-body.md"]}
        onCancel={() => {}}
        onImported={() => {}}
      />
    ));
    expect(getByText("01-intro.md")).toBeInTheDocument();
    expect(getByText("02-body.md")).toBeInTheDocument();
    expect(getByText("Order the chapters")).toBeInTheDocument();
  });

  it("cancel button fires onCancel without touching the backend", () => {
    const onCancel = vi.fn();
    const { getByRole } = render(() => (
      <ImportView
        directory="/tmp/src"
        files={["a.md"]}
        onCancel={onCancel}
        onImported={() => {}}
      />
    ));
    fireEvent.click(getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("does nothing if the user cancels the save dialog", async () => {
    mockSave.mockResolvedValueOnce(null);
    const onImported = vi.fn();
    const { getByRole } = render(() => (
      <ImportView
        directory="/tmp/src"
        files={["a.md"]}
        onCancel={() => {}}
        onImported={onImported}
      />
    ));
    fireEvent.click(getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(onImported).not.toHaveBeenCalled();
  });

  it("invokes import_book with the ordered files and fires onImported", async () => {
    mockSave.mockResolvedValueOnce("/tmp/dest/my-book.scholium");
    const manifest = { version: 1, metadata: {}, lessonPlan: { chapters: [] } };
    mockInvoke.mockResolvedValueOnce(manifest);
    const onImported = vi.fn();
    const { getByRole } = render(() => (
      <ImportView
        directory="/tmp/src"
        files={["a.md", "b.md"]}
        onCancel={() => {}}
        onImported={onImported}
      />
    ));
    fireEvent.click(getByRole("button", { name: "Confirm" }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "import_book",
        expect.objectContaining({
          sourceDir: "/tmp/src",
          destPath: "/tmp/dest/my-book.scholium",
          orderedFiles: ["a.md", "b.md"],
        }),
      );
    });
    await waitFor(() => expect(onImported).toHaveBeenCalledWith(manifest));
  });
});
