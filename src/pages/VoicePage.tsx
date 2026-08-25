import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Endpoint = {
  id: string;
  name: string;
  kind: "Output" | "Input";
};

type PersistedSettings = {
  selected_device_id: string | null;
  output_endpoint_id: string | null;
};

type VbCableStatus = {
  input: boolean;
  output: boolean;
  ready: boolean;
};

const FALLBACK_ENDPOINTS: Endpoint[] = [
  { id: "cable-input", name: "CABLE 输入（VB-CABLE）", kind: "Output" },
];

export function VoicePage() {
  const [endpoints, setEndpoints] = useState<Endpoint[]>(FALLBACK_ENDPOINTS);
  const [selected, setSelected] = useState("CABLE 输入（VB-CABLE）");
  const [toneResult, setToneResult] = useState("");
  const [vbCable, setVbCable] = useState<VbCableStatus | null>(null);
  const [vbMsg, setVbMsg] = useState("");
  const [installing, setInstalling] = useState(false);
  const [simResult, setSimResult] = useState("");

  async function refreshVbStatus() {
    if (!isTauri()) return;
    try {
      setVbCable(await invoke<VbCableStatus>("vb_cable_status"));
      return null;
    } catch {
      setVbCable(null);
      return null;
    }
  }

  useEffect(() => {
    if (!isTauri()) return;
    invoke<Endpoint[]>("list_audio_endpoints")
      .then(async (list) => {
        const eps = list.length ? list : FALLBACK_ENDPOINTS;
        setEndpoints(eps);
        let initial = eps[0]?.name || "";
        try {
          const saved = await invoke<PersistedSettings>("get_persisted_settings");
          if (saved.output_endpoint_id) {
            initial = saved.output_endpoint_id;
          }
        } catch {
          // ignore
        }
        setSelected(initial);
        return null;
      })
      .catch(() => {});
    refreshVbStatus();
  }, []);

  async function runVoiceSimulation() {
    if (!isTauri()) {
      setSimResult("浏览器预览：请在桌面应用内模拟");
      return;
    }
    setSimResult("正在模拟：Win+H → 合成语音 → CABLE…");
    try {
      const ret = await invoke<{
        frames: number;
        pcm_samples: number;
        output_samples: number;
        win_h_toast: boolean;
      }>("simulate_voice_chain", { outputDevice: selected });
      setSimResult(
        `模拟完成：${ret.frames} 帧，PCM ${ret.pcm_samples}，输出 ${ret.output_samples} 样本，Win+H=${ret.win_h_toast}`
      );
    } catch (err) {
      setSimResult(`模拟失败：${err}`);
    }
  }

  async function installVbCable() {
    if (!isTauri()) return;
    setInstalling(true);
    setVbMsg("正在安装…请留意 UAC 弹窗确认");
    try {
      const msg = await invoke<string>("install_vb_cable");
      setVbMsg(msg);
      await refreshVbStatus();
      return null;
    } catch (err) {
      setVbMsg(`安装流程失败：${err}`);
      return null;
    } finally {
      setInstalling(false);
    }
  }

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
            <small>{selected || "CABLE 输出（麦克风）"}</small>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-title">输出端点（来自 Rust 后端）</div>
        <select
          className="select"
          value={selected}
          onChange={(e) => {
            const value = e.currentTarget.value;
            setSelected(value);
            if (isTauri()) {
              invoke("save_output_endpoint", { endpointId: value }).catch(() => {});
            }
          }}
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

      <section className="card">
        <div className="card-title">虚拟声卡（VB-CABLE）</div>
        {!isTauri() ? (
          <p className="hint">浏览器预览：请在桌面应用内检测/安装。</p>
        ) : vbCable === null ? (
          <p className="hint">检测中…</p>
        ) : vbCable.ready ? (
          <p>✅ 已安装（CABLE 输入 / 输出就绪）</p>
        ) : (
          <div>
            <p>
              未检测到 VB-CABLE（输入={vbCable.input ? "有" : "无"}，
              输出={vbCable.output ? "有" : "无"}）。请安装虚拟声卡后，语音才能进入系统听写。
            </p>
            <div className="actions">
              <button className="btn primary" onClick={installVbCable} disabled={installing}>
                {installing ? "正在安装…" : "安装 VB-CABLE"}
              </button>
              <button className="btn" onClick={refreshVbStatus}>
                重新检测
              </button>
            </div>
            {vbMsg && <p className="hint">{vbMsg}</p>}
          </div>
        )}
      </section>

      <section className="card actions">
        <button className="btn primary" onClick={sendTestTone} disabled={!isTauri()}>
          发送 1 秒测试音
        </button>
        <button className="btn" onClick={runVoiceSimulation} disabled={!isTauri()}>
          模拟完整语音链（无遥控器）
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

      {simResult && (
        <section className="card">
          <div className="card-title">模拟语音链结果</div>
          <p>{simResult}</p>
        </section>
      )}
    </div>
  );
}
