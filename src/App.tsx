import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

function App() {
  const [backendStatus, setBackendStatus] = useState("checking...");

  useEffect(() => {
    invoke<string>("ping")
      .then(setBackendStatus)
      .catch((err) => setBackendStatus(`backend error: ${err}`));
  }, []);

  return (
    <main className="container">
      <h1>Remote Mic</h1>
      <p>Windows 无线麦 — 把小米蓝牙语音遥控器变成无线麦克风。</p>

      <div className="status-card">
        <strong>Backend Status</strong>
        <span>{backendStatus}</span>
      </div>

      <p className="hint">
        Rust 后端 / Tauri 2 / TypeScript + React 框架已搭建。
      </p>
    </main>
  );
}

export default App;
