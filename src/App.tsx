import WelcomeModal from "./components/WelcomeModal";
import "./App.css";

function App() {
  function handleNext(topic: string) {
    // Lesson generation not yet implemented
    console.log("Topic selected:", topic);
  }

  return (
    <main class="app-shell">
      <WelcomeModal onNext={handleNext} />
    </main>
  );
}

export default App;
