import { createSignal, Match, onCleanup, onMount, Show, Switch } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import WelcomeModal from "./components/WelcomeModal";
import OnboardingView from "./components/OnboardingView";
import BookView from "./components/BookView";
import ImportView from "./components/ImportView";
import SettingsModal from "./components/SettingsModal";
import ChatModal from "./components/ChatModal";
import type { Manifest } from "./types/manifest";
import "./App.css";

type Stage =
  | { name: "welcome" }
  | { name: "onboarding"; topic: string }
  | { name: "import"; directory: string; files: string[] }
  | { name: "book"; manifest: Manifest };

function App() {
  const [stage, setStage] = createSignal<Stage>({ name: "welcome" });
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [chatOpen, setChatOpen] = createSignal(false);

  onMount(() => {
    // The native "Settings…" menu item emits this event.
    const unlistenSettings = listen("open-settings", () => setSettingsOpen(true));
    // The native "Chat…" menu item (⌘K) emits this event.
    const unlistenChat = listen("open-chat", () => setChatOpen(true));
    onCleanup(() => {
      unlistenSettings.then((un) => un());
      unlistenChat.then((un) => un());
    });
  });

  function handleManifestChanged(updated: Manifest) {
    // If we're in book view, replace the manifest so BookView reflects changes.
    if (stage().name === "book") {
      setStage({ name: "book", manifest: updated });
    }
  }

  return (
    <main class="app-shell">
      <Show when={settingsOpen()}>
        <SettingsModal onClose={() => setSettingsOpen(false)} />
      </Show>
      <Show when={chatOpen()}>
        <ChatModal
          onClose={() => setChatOpen(false)}
          onManifestChanged={(m) => {
            handleManifestChanged(m);
          }}
        />
      </Show>
      <Switch>
        <Match when={stage().name === "welcome"}>
          <WelcomeModal
            onTopic={(topic) => setStage({ name: "onboarding", topic })}
            onBook={(manifest) => setStage({ name: "book", manifest })}
            onImport={(directory, files) => setStage({ name: "import", directory, files })}
          />
        </Match>
        <Match when={stage().name === "onboarding"}>
          <OnboardingView
            topic={(stage() as { name: "onboarding"; topic: string }).topic}
            onBook={(manifest) => setStage({ name: "book", manifest })}
            onBack={() => setStage({ name: "welcome" })}
          />
        </Match>
        <Match when={stage().name === "import"}>
          <ImportView
            directory={(stage() as { name: "import"; directory: string; files: string[] }).directory}
            files={(stage() as { name: "import"; directory: string; files: string[] }).files}
            onCancel={() => setStage({ name: "welcome" })}
            onImported={(manifest) => setStage({ name: "book", manifest })}
          />
        </Match>
        <Match when={stage().name === "book"}>
          <BookView
            manifest={(stage() as { name: "book"; manifest: Manifest }).manifest}
          />
        </Match>
      </Switch>
    </main>
  );
}

export default App;
