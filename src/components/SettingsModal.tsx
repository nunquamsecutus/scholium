import { createSignal, onMount, Show } from "solid-js";
import { invoke } from "@tauri-apps/api/core";

type Provider = "claude" | "ollama";
type ImageQuality = "fast" | "medium" | "high";

interface PublicSettings {
  provider: Provider;
  claudeConfigured: boolean;
  ollamaUrl: string;
  ollamaModel: string;
  imageProvider: Provider;
  claudeImageModel: string;
  ollamaImageModel: string;
  imageQuality: ImageQuality;
}

interface Props {
  onClose: () => void;
}

export default function SettingsModal(props: Props) {
  const [provider, setProvider] = createSignal<Provider>("ollama");
  const [claudeConfigured, setClaudeConfigured] = createSignal(false);
  const [claudeKey, setClaudeKey] = createSignal("");
  const [ollamaUrl, setOllamaUrl] = createSignal("");
  const [ollamaModel, setOllamaModel] = createSignal("");
  const [imageProvider, setImageProvider] = createSignal<Provider>("ollama");
  const [claudeImageModel, setClaudeImageModel] =
    createSignal("claude-sonnet-4-6");
  const [ollamaImageModel, setOllamaImageModel] = createSignal("llama3.2");
  const [imageQuality, setImageQuality] = createSignal<ImageQuality>("medium");
  const [saving, setSaving] = createSignal(false);
  const [error, setError] = createSignal("");

  onMount(async () => {
    try {
      const s = await invoke<PublicSettings>("get_settings");
      setProvider(s.provider);
      setClaudeConfigured(s.claudeConfigured);
      setOllamaUrl(s.ollamaUrl);
      setOllamaModel(s.ollamaModel);
      setImageProvider(s.imageProvider);
      setClaudeImageModel(s.claudeImageModel);
      setOllamaImageModel(s.ollamaImageModel);
      setImageQuality(s.imageQuality);
    } catch (e) {
      setError(String(e));
    }
  });

  function baseUpdate() {
    return {
      provider: provider(),
      ollamaUrl: ollamaUrl(),
      ollamaModel: ollamaModel(),
      imageProvider: imageProvider(),
      claudeImageModel: claudeImageModel(),
      ollamaImageModel: ollamaImageModel(),
      imageQuality: imageQuality(),
    };
  }

  async function save() {
    setSaving(true);
    setError("");
    const key = claudeKey().trim();
    try {
      const s = await invoke<PublicSettings>("update_settings", {
        update: { ...baseUpdate(), claudeApiKey: key ? key : null },
      });
      setClaudeConfigured(s.claudeConfigured);
      props.onClose();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  }

  async function removeKey() {
    setSaving(true);
    setError("");
    try {
      const s = await invoke<PublicSettings>("update_settings", {
        update: { ...baseUpdate(), clearClaudeKey: true },
      });
      setClaudeConfigured(s.claudeConfigured);
      setClaudeKey("");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      class="settings-backdrop"
      role="presentation"
      onClick={() => props.onClose()}
    >
      <div
        class="settings-modal"
        role="dialog"
        aria-label="Settings"
        onClick={(e) => e.stopPropagation()}
      >
        <div class="settings-header">
          <span class="settings-title">Settings</span>
          <button
            type="button"
            class="settings-close"
            aria-label="Close"
            onClick={() => props.onClose()}
          >
            ×
          </button>
        </div>

        <div class="settings-body">
          {error() && (
            <p class="field-error" role="alert">
              {error()}
            </p>
          )}

          <label class="settings-field">
            <span class="settings-label">LLM provider</span>
            <select
              value={provider()}
              onChange={(e) => setProvider(e.currentTarget.value as Provider)}
            >
              <option value="claude">Claude</option>
              <option value="ollama">Ollama</option>
            </select>
          </label>

          <fieldset class="settings-group">
            <legend>Claude</legend>
            <label class="settings-field">
              <span class="settings-label">
                API key
                <Show when={claudeConfigured()}>
                  <span class="settings-configured"> · stored</span>
                </Show>
              </span>
              <input
                type="password"
                autocomplete="off"
                placeholder={
                  claudeConfigured()
                    ? "•••••••• (leave blank to keep)"
                    : "sk-ant-…"
                }
                value={claudeKey()}
                onInput={(e) => setClaudeKey(e.currentTarget.value)}
              />
            </label>
            <Show when={claudeConfigured()}>
              <button
                type="button"
                class="settings-remove-key"
                onClick={removeKey}
                disabled={saving()}
              >
                Remove stored key
              </button>
            </Show>
          </fieldset>

          <fieldset class="settings-group">
            <legend>Ollama</legend>
            <label class="settings-field">
              <span class="settings-label">Server URL</span>
              <input
                type="text"
                value={ollamaUrl()}
                onInput={(e) => setOllamaUrl(e.currentTarget.value)}
              />
            </label>
            <label class="settings-field">
              <span class="settings-label">Model</span>
              <input
                type="text"
                value={ollamaModel()}
                onInput={(e) => setOllamaModel(e.currentTarget.value)}
              />
            </label>
          </fieldset>

          <fieldset class="settings-group">
            <legend>Image generation</legend>
            <label class="settings-field">
              <span class="settings-label">Provider</span>
              <select
                value={imageProvider()}
                onChange={(e) =>
                  setImageProvider(e.currentTarget.value as Provider)
                }
              >
                <option value="claude">Claude</option>
                <option value="ollama">Ollama</option>
              </select>
            </label>
            <Show when={imageProvider() === "claude"}>
              <label class="settings-field">
                <span class="settings-label">Model</span>
                <input
                  type="text"
                  placeholder="claude-sonnet-4-6"
                  value={claudeImageModel()}
                  onInput={(e) => setClaudeImageModel(e.currentTarget.value)}
                />
              </label>
            </Show>
            <Show when={imageProvider() === "ollama"}>
              <label class="settings-field">
                <span class="settings-label">Model</span>
                <input
                  type="text"
                  value={ollamaImageModel()}
                  onInput={(e) => setOllamaImageModel(e.currentTarget.value)}
                />
              </label>
            </Show>
          </fieldset>

          <label class="settings-field">
            <span class="settings-label">Image quality</span>
            <select
              value={imageQuality()}
              onChange={(e) =>
                setImageQuality(e.currentTarget.value as ImageQuality)
              }
            >
              <option value="fast">Fast</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </label>
        </div>

        <div class="settings-footer">
          <button
            type="button"
            class="settings-cancel"
            onClick={() => props.onClose()}
            disabled={saving()}
          >
            Cancel
          </button>
          <button
            type="button"
            class="btn-primary"
            onClick={save}
            disabled={saving()}
          >
            {saving() ? "Saving…" : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
}
