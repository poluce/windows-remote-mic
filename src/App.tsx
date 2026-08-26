import { useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ConnectionPage } from "./pages/ConnectionPage";
import { MappingPage } from "./pages/MappingPage";
import { VoicePage } from "./pages/VoicePage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { OnboardingPage } from "./pages/OnboardingPage";
import "./App.css";

function App() {
  const [page, setPage] = useState<PageId>("connection");

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} />

      <main className="main">
        <div className="content">
          {page === "connection" && <ConnectionPage />}
          {page === "mapping" && <MappingPage />}
          {page === "voice" && <VoicePage />}
          {page === "diagnostics" && <DiagnosticsPage />}
          {page === "guidance" && <OnboardingPage />}
        </div>
      </main>
    </div>
  );
}

export default App;
