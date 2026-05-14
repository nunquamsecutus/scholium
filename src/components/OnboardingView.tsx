import { createSignal, createEffect, For, Show } from "solid-js";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import type { Manifest } from "../types/manifest";
import type { ChatMessage, GeneratedPlan } from "../types/onboarding";

const READY_SIGNAL = "Ready to generate your lesson plan.";

interface Props {
  topic: string;
  onBook: (manifest: Manifest) => void;
  onBack: () => void;
}

export default function OnboardingView(props: Props) {
  const [messages, setMessages] = createSignal<ChatMessage[]>([]);
  const [input, setInput] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal("");
  const [plan, setPlan] = createSignal<GeneratedPlan | null>(null);
  const [saving, setSaving] = createSignal(false);

  const lastAssistantMessage = () => {
    const msgs = messages();
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === "assistant") return msgs[i].content;
    }
    return "";
  };

  const llmSignaledReady = () => lastAssistantMessage().includes(READY_SIGNAL);
  const canGeneratePlan = () => messages().some((m) => m.role === "assistant") && !loading();

  // Kick off the conversation as soon as the component mounts.
  createEffect(() => {
    void startOnboarding();
  });

  async function startOnboarding() {
    setLoading(true);
    setError("");
    try {
      const response = await invoke<string>("begin_onboarding", { topic: props.topic });
      setMessages([{ role: "assistant", content: response }]);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function sendMessage() {
    const text = input().trim();
    if (!text || loading()) return;
    setInput("");
    setError("");

    const updated: ChatMessage[] = [...messages(), { role: "user", content: text }];
    setMessages(updated);
    setLoading(true);

    try {
      const response = await invoke<string>("continue_onboarding", {
        topic: props.topic,
        conversation: updated,
      });
      setMessages([...updated, { role: "assistant", content: response }]);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleGeneratePlan() {
    setLoading(true);
    setError("");
    try {
      const generated = await invoke<GeneratedPlan>("generate_lesson_plan", {
        topic: props.topic,
        conversation: messages(),
      });
      setPlan(generated);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  async function handleSaveBook() {
    const currentPlan = plan();
    if (!currentPlan) return;
    setError("");

    const dialogPath = await save({
      title: "Save Book",
      filters: [{ name: "Edu Book", extensions: ["edubook"] }],
    });
    if (!dialogPath) return;

    setSaving(true);
    try {
      const manifest = await invoke<Manifest>("create_book", {
        topic: props.topic,
        dialogPath,
        plan: currentPlan,
      });
      props.onBook(manifest);
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  return (
    <div class="onboarding-shell">
      <header class="onboarding-header">
        <button class="btn-back" onClick={props.onBack} disabled={loading() || saving()}>
          ← Back
        </button>
        <div class="onboarding-topic">
          <span class="onboarding-topic-label">Topic:</span>
          <span class="onboarding-topic-value">{props.topic}</span>
        </div>
      </header>

      <Show when={!plan()} fallback={<PlanPreview plan={plan()!} onSave={handleSaveBook} saving={saving()} />}>
        <div class="chat-area">
          <div class="chat-messages" id="chat-messages">
            <Show when={messages().length === 0 && loading()}>
              <div class="chat-loading">Starting conversation…</div>
            </Show>
            <For each={messages()}>
              {(msg) => (
                <div class={`chat-bubble chat-bubble--${msg.role}`}>
                  <p>{msg.content}</p>
                </div>
              )}
            </For>
            <Show when={loading() && messages().length > 0}>
              <div class="chat-bubble chat-bubble--assistant chat-bubble--thinking">
                <span class="thinking-dot" /><span class="thinking-dot" /><span class="thinking-dot" />
              </div>
            </Show>
          </div>

          {error() && <p class="field-error" role="alert">{error()}</p>}

          <div class="chat-input-row">
            <textarea
              class="chat-input"
              rows={2}
              placeholder="Type your response…"
              value={input()}
              onInput={(e) => setInput(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); void sendMessage(); }
              }}
              disabled={loading() || messages().length === 0}
            />
            <button
              class="btn-send"
              onClick={sendMessage}
              disabled={input().trim().length === 0 || loading() || messages().length === 0}
            >
              Send
            </button>
          </div>

          <div class="chat-actions">
            <button
              class="btn-primary"
              disabled={!canGeneratePlan()}
              onClick={handleGeneratePlan}
            >
              {llmSignaledReady() ? "Generate Lesson Plan ✓" : "Generate Lesson Plan"}
            </button>
          </div>
        </div>
      </Show>
    </div>
  );
}

interface PlanPreviewProps {
  plan: GeneratedPlan;
  onSave: () => void;
  saving: boolean;
}

function PlanPreview(props: PlanPreviewProps) {
  return (
    <div class="plan-preview">
      <h2 class="plan-preview-title">Your Lesson Plan</h2>
      <p class="plan-description">{props.plan.description}</p>
      <dl class="plan-meta">
        <dt>Level</dt><dd>{props.plan.readingLevel}</dd>
        <dt>Background</dt><dd>{props.plan.priorKnowledge}</dd>
      </dl>
      <ol class="plan-chapters">
        <For each={props.plan.lessonPlan.chapters}>
          {(ch) => (
            <li class="plan-chapter">
              <strong>{ch.title}</strong>
              <span>{ch.description}</span>
            </li>
          )}
        </For>
      </ol>
      <button class="btn-primary" onClick={props.onSave} disabled={props.saving}>
        {props.saving ? "Saving…" : "Save Book"}
      </button>
    </div>
  );
}
