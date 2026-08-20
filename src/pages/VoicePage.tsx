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
  const [selected, setSelected] = useState("CABLE Input (VB-CABLE)");
  const [toneResult, setToneResult] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    invoke<Endpoint[]>("list_audio_endpoints")
      .then((list) => {
        const eps = list.length ? list : FALLBACK_ENDPOINTS;
        setEndpoints(eps);
        setSelected((prev) => prev || eps[0]?.name || "");
        return null;
      })
      .catch(() => {});
  }, []);

  async function sendTestTone() {
    setToneResult("播放中…");
    try {
      const result = await invoke<string>("play_test_tone", {
        deviceName: selected,
      });
      setToneResult(result);
    } catch (err) {
      setToneResult(`失败: ${err}`);
    }
  }

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
            <option key={ep.id} value={ep.name}>
              {ep.name}
            </option>
          ))}
        </select>
        {!isTauri() && (
          <p className="hint">浏览器预览：显示占位端点；桌面应用内会列出真实 WASAPI 设备。</p>
        )}
      </section>

      <section className="card actions">
        <button className="btn primary" onClick={sendTestTone} disabled={!isTauri()}>
          发送 1 秒测试音
        </button>
        <button className="btn" disabled>
          打开系统语音设置
        </button>
      </section>

      {toneResult && (
        <section className="card">
          <div className="card-title">测试音结果</div>
          <p>{toneResult}</p>
        </section>
      )}
    </div>
  );
}
