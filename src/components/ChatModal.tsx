import {
  createSignal,
  createEffect,
  For,
  Show,
  onMount,
  onCleanup,
} from "solid-js";
import { invoke } from "@tauri-apps/api/core";
import type { Manifest } from "../types/manifest";

// ── Types ─────────────────────────────────────────────────────────────────────

interface ChatMessage {
  role: "user" | "assistant";
  content: string;
}

interface ManifestPatch {
  action: string;
  [key: string]: unknown;
}

interface ChatReply {
  reply: string;
  patch: ManifestPatch | null;
}

// Human-readable descriptions for each patch action.
function describePatch(patch: ManifestPatch): string {
  switch (patch.action) {
    case "rename_chapter":
      return `Rename chapter "${patch.id}" to "${patch.title}"`;
    case "reorder_chapters":
      return `Reorder chapters: ${(patch.ids as string[]).join(" → ")}`;
    case "add_chapter":
      return `Add new chapter "${patch.title}"${patch.after_id ? ` after "${patch.after_id}"` : " at the start"}`;
    case "remove_chapter":
      return `Remove chapter "${patch.id}"`;
    case "merge_chapters":
      return `Merge chapter "${patch.source_id}" into "${patch.target_id}" as "${patch.title}"`;
    case "split_chapter":
      return `Split chapter "${patch.id}" into ${(patch.new_chapters as { title: string }[]).length} chapters`;
    case "update_metadata":
      return "Update book metadata";
    default:
      return `Apply change: ${patch.action}`;
  }
}

// Strip the ```json ... ``` block from the displayed message so the user
// sees clean prose (the patch UI handles the structured part).
function stripPatchBlock(text: string): string {
  return text.replace(/```json[\s\S]*?```/g, "").trim();
}

// ── Props ─────────────────────────────────────────────────────────────────────

interface Props {
  onClose: () => void;
  /** Notified when a patch is applied so BookView can refresh. */
  onManifestChanged: (manifest: Manifest) => void;
}

// ── Component ─────────────────────────────────────────────────────────────────

export default function ChatModal(props: Props) {
  const [history, setHistory] = createSignal<ChatMessage[]>([]);
  const [input, setInput] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal("");
  const [pendingPatch, setPendingPatch] = createSignal<ManifestPatch | null>(null);
  const [applyError, setApplyError] = createSignal("");

  let threadEl: HTMLDivElement | undefined;
  let inputEl: HTMLTextAreaElement | undefined;

  // Scroll to bottom whenever the message list changes.
  createEffect(() => {
    history(); // track
    if (threadEl) {
      threadEl.scrollTop = threadEl.scrollHeight;
    }
  });

  onMount(() => {
    inputEl?.focus();
  });

  // Close on Escape.
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") props.onClose();
  }
  document.addEventListener("keydown", handleKeyDown);
  onCleanup(() => document.removeEventListener("keydown", handleKeyDown));

  async function send() {
    const text = input().trim();
    if (!text || busy()) return;

    const userMsg: ChatMessage = { role: "user", content: text };
    setHistory((h) => [...h, userMsg]);
    setInput("");
    setBusy(true);
    setError("");
    setPendingPatch(null);
    setApplyError("");

    try {
      const res = await invoke<ChatReply>("chat_about_book", {
        history: [...history()],
      });

      const assistantMsg: ChatMessage = {
        role: "assistant",
        content: res.reply,
      };
      setHistory((h) => [...h, assistantMsg]);

      if (res.patch) {
        setPendingPatch(res.patch);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      // Re-focus input after reply.
      setTimeout(() => inputEl?.focus(), 0);
    }
  }

  function handleInputKeyDown(e: KeyboardEvent) {
    // Send on Enter (not Shift+Enter, which inserts a newline).
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function applyPatch() {
    const patch = pendingPatch();
    if (!patch) return;
    setApplyError("");
    try {
      const updated = await invoke<Manifest>("apply_manifest_patch", { patch });
      setPendingPatch(null);
      props.onManifestChanged(updated);
      // Acknowledge in the thread.
      setHistory((h) => [
        ...h,
        {
          role: "assistant" as const,
          content: `✓ Change applied: ${describePatch(patch)}`,
        },
      ]);
    } catch (e) {
      setApplyError(String(e));
    }
  }

  function rejectPatch() {
    setPendingPatch(null);
    setApplyError("");
    setHistory((h) => [
      ...h,
      { role: "assistant" as const, content: "OK, the change was not applied." },
    ]);
  }

  return (
    <div
      class="chat-backdrop"
      role="presentation"
      onClick={() => props.onClose()}
    >
      <div
        class="chat-modal"
        role="dialog"
        aria-label="Chat with AI"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div class="chat-header">
          <span class="chat-title">Chat</span>
          <button
            type="button"
            class="chat-close"
            aria-label="Close"
            onClick={() => props.onClose()}
          >
            ×
          </button>
        </div>

        {/* Message thread */}
        <div class="chat-thread" ref={threadEl}>
          <Show
            when={history().length > 0}
            fallback={
              <p class="chat-empty">
                Ask me anything about this book, or ask me to add, remove,
                rename, reorder, merge, or split chapters.
              </p>
            }
          >
            <For each={history()}>
              {(msg) => (
                <div
                  class={`chat-bubble chat-bubble--${msg.role}`}
                >
                  <Show
                    when={msg.role === "assistant"}
                    fallback={<p class="chat-text">{msg.content}</p>}
                  >
                    <p class="chat-text">{stripPatchBlock(msg.content)}</p>
                  </Show>
                </div>
              )}
            </For>
          </Show>

          {/* Pending-patch confirmation banner (shown inside thread) */}
          <Show when={pendingPatch()}>
            {(patch) => (
              <div class="chat-patch-banner">
                <p class="chat-patch-description">{describePatch(patch())}</p>
                <Show when={applyError()}>
                  <p class="chat-patch-error" role="alert">{applyError()}</p>
                </Show>
                <div class="chat-patch-actions">
                  <button
                    type="button"
                    class="chat-patch-accept"
                    onClick={applyPatch}
                  >
                    Apply
                  </button>
                  <button
                    type="button"
                    class="chat-patch-reject"
                    onClick={rejectPatch}
                  >
                    Dismiss
                  </button>
                </div>
              </div>
            )}
          </Show>

          {/* Typing indicator */}
          <Show when={busy()}>
            <div class="chat-bubble chat-bubble--assistant chat-bubble--typing">
              <span class="chat-dot" />
              <span class="chat-dot" />
              <span class="chat-dot" />
            </div>
          </Show>
        </div>

        {/* Error */}
        <Show when={error()}>
          <p class="chat-error" role="alert">{error()}</p>
        </Show>

        {/* Input area */}
        <div class="chat-input-row">
          <textarea
            ref={inputEl}
            class="chat-input"
            placeholder="Message… (Enter to send, Shift+Enter for newline)"
            rows={1}
            value={input()}
            onInput={(e) => setInput(e.currentTarget.value)}
            onKeyDown={handleInputKeyDown}
            disabled={busy()}
          />
          <button
            type="button"
            class="chat-send"
            onClick={send}
            disabled={busy() || !input().trim()}
            aria-label="Send"
          >
            ↑
          </button>
        </div>
      </div>
    </div>
  );
}
