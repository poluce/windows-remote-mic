import { useEffect, useState } from "react";
import { Sidebar, type PageId } from "./components/Sidebar";
import { ConnectionPage } from "./pages/ConnectionPage";
import { MappingPage } from "./pages/MappingPage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";
import { OnboardingPage } from "./pages/OnboardingPage";
import { initRuntimeStatus } from "./store/runtimeStatus";
import "./App.css";

function App() {
  const [page, setPage] = useState<PageId>("connection");

  useEffect(() => {
    // 全局运行时状态初始化：事件监听不随页面切换而销毁。
    initRuntimeStatus();
  }, []);

  return (
    <div className="app-shell">
      <Sidebar page={page} onChange={setPage} />

      <main className="main">
        <div className="content">
          {page === "connection" && <ConnectionPage />}
          {page === "mapping" && <MappingPage />}
          {page === "diagnostics" && <DiagnosticsPage />}
          {page === "guidance" && <OnboardingPage />}
        </div>
      </main>
    </div>
  );
}

export default App;
