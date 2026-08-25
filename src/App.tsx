import { useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ConnectionPage } from "./pages/ConnectionPage";
import { MappingPage } from "./pages/MappingPage";
import { VoicePage } from "./pages/VoicePage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { OnboardingPage } from "./pages/OnboardingPage";
import { StatsPage } from "./pages/StatsPage";
import "./App.css";

function App() {
  const [page, setPage] = useState<PageId>("connection");

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} />

      <main className="main">
        <header className="topbar">
          <div>
            <h1>{pageTitle(page)}</h1>
          </div>
        </header>

        <div className="content">
          {page === "connection" && <ConnectionPage />}
          {page === "mapping" && <MappingPage />}
          {page === "voice" && <VoicePage />}
          {page === "diagnostics" && <DiagnosticsPage />}
          {page === "guidance" && <OnboardingPage />}
          {page === "stats" && <StatsPage />}
        </div>
      </main>
    </div>
  );
}

function pageTitle(page: PageId): string {
  switch (page) {
    case "connection":
      return "Remote Mic";
    case "mapping":
      return "按键映射";
    case "voice":
      return "语音";
    case "diagnostics":
      return "诊断";
    case "guidance":
      return "引导";
    case "stats":
      return "统计";
  }
}

export default App;
