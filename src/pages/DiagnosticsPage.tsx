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

type LogFileInfo = {
  name: string;
  path: string;
  size: number;
};

type SelfTestItem = {
  name: string;
  status: "pass" | "fail" | "skip";
  detail: string;
};

export function DiagnosticsPage() {
  const [data, setData] = useState<Diagnostics>(EMPTY);
  const [status, setStatus] = useState("请在桌面应用内运行检查");
  const [looping, setLooping] = useState(false);
  const [selfTests, setSelfTests] = useState<SelfTestItem[] | null>(null);
  const [logs, setLogs] = useState<LogFileInfo[]>([]);
  const [logContent, setLogContent] = useState("");

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

  async function loadLogs() {
    if (!isTauri()) return;
    try {
      setLogs(await invoke<LogFileInfo[]>("list_log_files"));
    } catch {
      setLogs([]);
    }
  }

  async function viewLog(path: string) {
    if (!isTauri()) return;
    try {
      setLogContent(await invoke<string>("read_log_file", { path }));
    } catch (err) {
      setLogContent(`读取失败：${err}`);
    }
  }

  useEffect(() => {
    loadLogs();
  }, []);

  async function runSelfTest() {
    if (!isTauri()) {
      setStatus("浏览器预览：请在桌面应用内运行自检");
      return;
    }
    try {
      setStatus("正在运行自检…");
      setSelfTests(await invoke<SelfTestItem[]>("run_self_test"));
      setStatus("自检完成");
    } catch (err) {
      setStatus(`自检失败: ${err}`);
    }
  }

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
            <div className="brief-label">CABLE 输入</div>
          </div>
          <div className={`brief ${data.cable_output_present ? "ok" : "warn"}`}>
            <div className="brief-value">{data.cable_output_present ? "有" : "无"}</div>
            <div className="brief-label">CABLE 输出（麦克风）</div>
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
                <span className="badge badge-ok">输出</span>
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
                <span className="badge badge-warn">输入</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="card">
        <div className="card-title">检测状态</div>
        <p>{status}</p>
      </section>

      <section className="card">
        <div className="card-title">能力自检（PASS/FAIL）</div>
        {!selfTests ? (
          <p className="hint">点击下方「运行自检」验证各真实模块。</p>
        ) : (
          <div className="check-list">
            {selfTests.map((t) => (
              <div key={t.name} className="check-row">
                <span>{t.name}</span>
                <div>
                  <span className={`badge badge-${t.status === "pass" ? "ok" : t.status === "fail" ? "err" : "warn"}`}>
                    {t.status.toUpperCase()}
                  </span>
                  {t.detail && <span className="hint"> {t.detail}</span>}
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="card">
        <div className="card-title">日志 / 抓包文件</div>
        <div className="actions">
          <button className="btn" onClick={loadLogs}>刷新日志</button>
        </div>
        {logs.length === 0 ? (
          <p className="hint">暂无日志文件。</p>
        ) : (
          <div className="check-list">
            {logs.map((l) => (
              <div key={l.path} className="check-row">
                <button className="btn small" onClick={() => viewLog(l.path)}>
                  {l.name} · {l.size} B
                </button>
              </div>
            ))}
          </div>
        )}
        {logContent && (
          <pre className="log-preview">{logContent}</pre>
        )}
      </section>

      <section className="card actions">
        <button className="btn primary" onClick={runCheck}>
          运行检查
        </button>
        <button className="btn primary" onClick={runSelfTest}>
          运行自检
        </button>
        <button className="btn" onClick={loopTone} disabled={looping}>
          {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
        </button>
      </section>
    </div>
  );
}
