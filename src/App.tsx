import { createSignal, Match, Switch } from "solid-js";
import WelcomeModal from "./components/WelcomeModal";
import OnboardingView from "./components/OnboardingView";
import BookView from "./components/BookView";
import type { Manifest } from "./types/manifest";
import "./App.css";

type Stage =
  | { name: "welcome" }
  | { name: "onboarding"; topic: string }
  | { name: "book"; manifest: Manifest };

function App() {
  const [stage, setStage] = createSignal<Stage>({ name: "welcome" });

  return (
    <main class="app-shell">
      <Switch>
        <Match when={stage().name === "welcome"}>
          <WelcomeModal
            onTopic={(topic) => setStage({ name: "onboarding", topic })}
            onBook={(manifest) => setStage({ name: "book", manifest })}
          />
        </Match>
        <Match when={stage().name === "onboarding"}>
          <OnboardingView
            topic={(stage() as { name: "onboarding"; topic: string }).topic}
            onBook={(manifest) => setStage({ name: "book", manifest })}
            onBack={() => setStage({ name: "welcome" })}
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
