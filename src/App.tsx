import { createSignal, Show } from "solid-js";
import WelcomeModal from "./components/WelcomeModal";
import BookView from "./components/BookView";
import type { Manifest } from "./types/manifest";
import "./App.css";

function App() {
  const [manifest, setManifest] = createSignal<Manifest | null>(null);

  return (
    <main class="app-shell">
      <Show
        when={manifest()}
        fallback={<WelcomeModal onBook={(m) => setManifest(m)} />}
      >
        {(m) => <BookView manifest={m()} />}
      </Show>
    </main>
  );
}

export default App;
