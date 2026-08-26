import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type Diagnostics = {
  has_vb_cable: boolean;
  cable_input_present: boolean;
  cable_output_present: boolean;
};

const EMPTY: Diagnostics = {
  has_vb_cable: false,
  cable_input_present: false,
  cable_output_present: false,
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
    setLooping(true);
    setStatus("循环播放测试音中…");
    try {
      const result = await invoke<string>("play_test_tone_loop", {
        deviceName: "CABLE Input",
        repetitions: 3,
      });
      setStatus(result);
    } catch (err) {
      setStatus(`播放失败: ${err}`);
    } finally {
      setLooping(false);
    }
  }

  function toggleQuickMenu() {
    if (!isTauri()) return;
    invoke("toggle_quick_menu");
  }

  return (
    <div className="page">

      <section className="card">
        <div className="card-title">虚拟声卡（VB-CABLE）</div>
        <div className="brief-grid">
          <div className={`brief ${data.has_vb_cable ? "ok" : "warn"}`}>
            <div className="brief-value">{data.has_vb_cable ? "正常" : "未就绪"}</div>
            <div className="brief-label">VB-CABLE 链路</div>
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
        <div className="card-title">快捷菜单调试</div>
        <p className="hint">临时测试入口：显示/隐藏右下角扇形快捷菜单。</p>
        <div className="actions">
          <button className="btn" onClick={toggleQuickMenu}>
            打开/关闭快捷菜单
          </button>
        </div>
      </section>

      <section className="card">
        <div className="actions">
          <button className="btn primary" onClick={runCheck}>
            运行检查
          </button>
          <button className="btn primary" onClick={runSelfTest}>
            运行自检
          </button>
          <button className="btn" onClick={loopTone} disabled={looping}>
            {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
          </button>
        </div>
        {status && <p className="hint">{status}</p>}
        {selfTests && (
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
    </div>
  );
}
