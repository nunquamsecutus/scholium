import { invoke } from "@tauri-apps/api/core";

/**
 * Speak `text` aloud.
 *
 * On macOS, delegates to the Rust `speak_text` command which pipes text to the
 * system `say` binary.  On other platforms (or if the command fails for any
 * reason), falls back to the browser's Web Speech API so the feature degrades
 * gracefully instead of being silently absent.
 */
export async function speakText(text: string): Promise<void> {
  const trimmed = text.trim();
  if (!trimmed) return;

  try {
    await invoke("speak_text", { text: trimmed });
  } catch (e) {
    // "platform_not_supported" is the expected signal from non-macOS builds.
    // Any other error also falls back so the user still hears something.
    if (typeof window !== "undefined" && window.speechSynthesis) {
      window.speechSynthesis.cancel();
      window.speechSynthesis.speak(new SpeechSynthesisUtterance(trimmed));
    }
  }
}

/**
 * Stop any in-progress speech — both the `say` subprocess (via Rust) and the
 * Web Speech API utterance queue.
 */
export async function stopSpeaking(): Promise<void> {
  try {
    await invoke("stop_speaking");
  } catch {
    // ignore — not fatal if the command fails
  }
  if (typeof window !== "undefined" && window.speechSynthesis) {
    window.speechSynthesis.cancel();
  }
}
