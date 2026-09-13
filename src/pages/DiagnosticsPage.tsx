import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { RemoteKeyTester } from "../components/RemoteKeyTester";
import { IconSearch } from "../components/icons";

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
  const [checked, setChecked] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMsg, setInstallMsg] = useState("");
  const [looping, setLooping] = useState(false);
  const [selfTests, setSelfTests] = useState<SelfTestItem[] | null>(null);
  const [vhidBusy, setVhidBusy] = useState(false);
  const [vhidMsg, setVhidMsg] = useState("");

  async function runCheck() {
    if (!isTauri()) {
      setStatus("浏览器预览：无法调用后端，请在桌面应用内运行检查");
      setChecked(true);
      return;
    }
    try {
      setData(await invoke<Diagnostics>("audio_diagnostics"));
      setStatus("检查完成");
    } catch (err) {
      setStatus(`检查失败: ${err}`);
    } finally {
      setChecked(true);
    }
  }

  useEffect(() => {
    runCheck();
  }, []);

  async function installVbCable() {
    if (!isTauri()) {
      setInstallMsg("请在桌面应用内一键安装");
      return;
    }
    setInstalling(true);
    setInstallMsg("正在安装…请留意 UAC 弹窗确认");
    try {
      const msg = await invoke<string>("install_vb_cable");
      setInstallMsg(msg);
      await runCheck();
    } catch (err) {
      setInstallMsg(`安装失败：${err}`);
    } finally {
      setInstalling(false);
    }
  }

  async function installVhidDriver() {
    if (!isTauri()) {
      setVhidMsg("请在桌面应用内操作");
      return;
    }
    setVhidBusy(true);
    setVhidMsg("正在安装虚拟键盘驱动…请在 UAC 弹窗中点「是」");
    try {
      setVhidMsg(await invoke<string>("install_vhid_driver"));
    } catch (err) {
      setVhidMsg(`安装失败：${err}`);
    } finally {
      setVhidBusy(false);
    }
  }

  async function runSelfTest() {
    if (!isTauri()) {
      setStatus("浏览器预览：请在桌面应用内运行自检");
      return;
    }
    try {
      setStatus("正在运行系统全自检…");
      // 联动刷新声卡端点状态
      await runCheck();
      setSelfTests(await invoke<SelfTestItem[]>("run_self_test"));
      setStatus("全自检完成，系统各项指标正常");
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
      <div className="section-label">状态与自检</div>
      <div className="diag-grid">
      <section className="card vbcable-card">
        <span className="vbcable-title">虚拟声卡（VB-CABLE）</span>
        <span className={`vbcable-status ${data.has_vb_cable ? "ok" : "warn"}`}>
          <span className="pill-dot" />
          {data.has_vb_cable ? "正常" : "未就绪"}
        </span>
        <span className={`vbcable-status ${data.cable_input_present ? "ok" : "warn"}`}>
          <span className="pill-dot" />
          CABLE 输入
        </span>
        <span className={`vbcable-status ${data.cable_output_present ? "ok" : "warn"}`}>
          <span className="pill-dot" />
          CABLE 输出
        </span>
        {checked && !data.has_vb_cable && (
          <button className="btn primary" onClick={installVbCable} disabled={installing || !isTauri()}>
            {installing ? "正在安装…" : "一键安装 VB-CABLE"}
          </button>
        )}
        {installMsg && <span className="hint vbcable-msg">{installMsg}</span>}
      </section>

      <section className="card diag-actions-card">
        <button className="btn primary" onClick={runSelfTest}>
          <IconSearch size={14} />
          运行系统全自检
        </button>
        <button className="btn" onClick={loopTone} disabled={looping}>
          {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
        </button>
        <button className="btn" onClick={toggleQuickMenu}>
          打开/关闭快捷菜单
        </button>
        <button className="btn" onClick={installVhidDriver} disabled={vhidBusy || !isTauri()}>
          {vhidBusy ? "正在安装…" : "安装 / 修复虚拟键盘驱动"}
        </button>
        {vhidMsg && <span className="hint diag-actions-msg">{vhidMsg}</span>}
        {status && <span className="hint diag-actions-msg">{status}</span>}
        {selfTests && (
          <div className="check-list diag-actions-list">
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

      <div className="section-label">按键测试</div>
      <RemoteKeyTester />
    </div>
  );
}
