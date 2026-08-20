import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Endpoint = {
  id: string;
  name: string;
  kind: "Output" | "Input";
};

type Diagnostics = {
  output_endpoints: Endpoint[];
  input_endpoints: Endpoint[];
  has_vb_cable: boolean;
  cable_input_present: boolean;
  cable_output_present: boolean;
};

const EMPTY: Diagnostics = {
  output_endpoints: [],
  input_endpoints: [],
  has_vb_cable: false,
  cable_input_present: false,
  cable_output_present: false,
};

export function DiagnosticsPage() {
  const [data, setData] = useState<Diagnostics>(EMPTY);
  const [status, setStatus] = useState("请在桌面应用内运行检查");
  const [looping, setLooping] = useState(false);

  async function runCheck() {
    if (!isTauri()) {
      setStatus("浏览器预览：无法调用后端，请在桌面应用内运行检查");
      return;
    }
    try {
      setData(await invoke<Diagnostics>("audio_diagnostics"));
      setStatus("检查完成");
    } catch (err) {
      setStatus(`检查失败: ${err}`);
    }
  }

  useEffect(() => {
    runCheck();
  }, []);

  async function loopTone() {
    if (!isTauri()) {
      setStatus("测试音循环仅在桌面应用内可用");
      return;
    }
    const device = data.output_endpoints[0]?.name ?? null;
    setLooping(true);
    setStatus("循环播放测试音中…");
    try {
      const result = await invoke<string>("play_test_tone_loop", {
        deviceName: device,
        repetitions: 3,
      });
      setStatus(result);
    } catch (err) {
      setStatus(`播放失败: ${err}`);
    } finally {
      setLooping(false);
    }
  }

  return (
    <div className="page">
      <h2>诊断</h2>
      <p className="page-sub">检查蓝牙、按键、虚拟声卡和系统语音链路。</p>

      <section className="card">
        <div className="card-title">虚拟声卡（VB-CABLE）</div>
        <div className="brief-grid">
          <div className={`brief ${data.has_vb_cable ? "ok" : "warn"}`}>
            <div className="brief-value">{data.has_vb_cable ? "正常" : "未就绪"}</div>
            <div className="brief-label">VB-CABLE 链路</div>
          </div>
          <div className={`brief ${data.cable_input_present ? "ok" : "warn"}`}>
            <div className="brief-value">{data.output_endpoints.length}</div>
            <div className="brief-label">输出设备数</div>
          </div>
          <div className={`brief ${data.cable_input_present ? "ok" : "warn"}`}>
            <div className="brief-value">{data.cable_input_present ? "有" : "无"}</div>
            <div className="brief-label">CABLE Input</div>
          </div>
          <div className={`brief ${data.cable_output_present ? "ok" : "warn"}`}>
            <div className="brief-value">{data.cable_output_present ? "有" : "无"}</div>
            <div className="brief-label">CABLE Output（麦克风）</div>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-title">输出设备（播放端点）</div>
        {data.output_endpoints.length === 0 ? (
          <p className="hint">暂无输出设备</p>
        ) : (
          <ul className="endpoint-list">
            {data.output_endpoints.map((ep) => (
              <li key={ep.id}>
                <span>{ep.name}</span>
                <span className="badge badge-ok">{ep.kind}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="card">
        <div className="card-title">输入设备（录音/麦克风端点）</div>
        {data.input_endpoints.length === 0 ? (
          <p className="hint">暂无输入设备</p>
        ) : (
          <ul className="endpoint-list">
            {data.input_endpoints.map((ep) => (
              <li key={ep.id}>
                <span>{ep.name}</span>
                <span className="badge badge-warn">{ep.kind}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="card">
        <div className="card-title">检测状态</div>
        <p>{status}</p>
      </section>

      <section className="card actions">
        <button className="btn primary" onClick={runCheck}>
          运行检查
        </button>
        <button className="btn" onClick={loopTone} disabled={looping}>
          {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
        </button>
      </section>
    </div>
  );
}
