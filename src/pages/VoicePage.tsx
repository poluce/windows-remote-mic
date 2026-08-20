import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Endpoint = {
  id: string;
  name: string;
  kind: "Output" | "Input";
};

const FALLBACK_ENDPOINTS: Endpoint[] = [
  { id: "cable-input", name: "CABLE Input (VB-CABLE)", kind: "Output" },
];

export function VoicePage() {
  const [endpoints, setEndpoints] = useState<Endpoint[]>(FALLBACK_ENDPOINTS);
  const [selected, setSelected] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    invoke<Endpoint[]>("list_audio_endpoints")
      .then((list) => {
        setEndpoints(list.length ? list : FALLBACK_ENDPOINTS);
        setSelected((prev) => prev || list[0]?.id || "");
        return null;
      })
      .catch(() => {});
  }, []);

  return (
    <div className="page">
      <h2>语音</h2>
      <p className="page-sub">
        优先使用 Windows 自带语音输入（Win+H），无需安装第三方输入法。
      </p>

      <section className="card">
        <div className="card-title">语音识别目标</div>
        <div className="voice-route">
          <div className="route-node active">
            <span>🎙️</span>
            <div>Windows 语音键入</div>
            <small>Win + H</small>
          </div>
          <span className="route-arrow">→</span>
          <div className="route-node">
            <span>🎧</span>
            <div>虚拟声卡</div>
            <small>{selected || "CABLE Output"}</small>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-title">输出端点（来自 Rust 后端）</div>
        <select
          className="select"
          value={selected}
          onChange={(e) => setSelected(e.currentTarget.value)}
          disabled={!isTauri()}
        >
          {endpoints.map((ep) => (
            <option key={ep.id} value={ep.id}>
              {ep.name}
            </option>
          ))}
        </select>
        {!isTauri() && (
          <p className="hint">浏览器预览：显示占位端点；桌面应用内会列出真实 WASAPI 设备。</p>
        )}
      </section>

      <section className="card actions">
        <button className="btn primary" disabled>
          发送 1 秒测试音
        </button>
        <button className="btn" disabled>
          打开系统语音设置
        </button>
      </section>
    </div>
  );
}
