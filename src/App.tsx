import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ConnectionPage } from "./pages/ConnectionPage";
import { MappingPage } from "./pages/MappingPage";
import { VoicePage } from "./pages/VoicePage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import "./App.css";

function App() {
  const [page, setPage] = useState<PageId>("connection");
  const [backend, setBackend] = useState("检查后端…");

  useEffect(() => {
    if (!isTauri()) {
      setBackend("浏览器预览模式");
      return;
    }
    invoke<string>("ping")
      .then(setBackend)
      .catch(() => setBackend("后端不可用"));
  }, []);

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} />

      <main className="main">
        <header className="topbar">
          <div>
            <h1>{pageTitle(page)}</h1>
          </div>
          <div className="backend-status">
            <span className="dot" />
            后端状态：{backend}
          </div>
        </header>

        <div className="content">
          {page === "connection" && <ConnectionPage />}
          {page === "mapping" && <MappingPage />}
          {page === "voice" && <VoicePage />}
          {page === "diagnostics" && <DiagnosticsPage />}
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
  }
}

export default App;
