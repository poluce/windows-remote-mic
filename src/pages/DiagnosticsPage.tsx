import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { RemoteKeyTester } from "../components/RemoteKeyTester";
import { IconSearch } from "../components/icons";
import { OUTPUT_DEVICE } from "../constants";
import "./DiagnosticsPage.css";

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

/// 前台应用与命中的应用配置（对应后端 core_dispatch::ForegroundStatus）。
type ForegroundStatus = {
  process: string | null;
  title: string | null;
  profile: string | null;
  profile_count: number;
  overridden_buttons: string[];
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
  const [reportMsg, setReportMsg] = useState("");
  const [foreground, setForeground] = useState<ForegroundStatus | null>(null);
  const [profileMsg, setProfileMsg] = useState("");

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
    refreshForeground();
  }, []);

  async function refreshForeground() {
    if (!isTauri()) return;
    try {
      setForeground(await invoke<ForegroundStatus>("app_profile_status"));
    } catch {
      // 后端暂不可用时保持上一次结果
    }
  }

  async function reloadProfiles() {
    if (!isTauri()) {
      setProfileMsg("请在桌面应用内操作");
      return;
    }
    try {
      const count = await invoke<number>("reload_app_profiles");
      await refreshForeground();
      setProfileMsg(`已重载 ${count} 份应用配置`);
    } catch (err) {
      setProfileMsg(`重载失败：${err}`);
    }
  }

  async function installVbCable() {
    if (!isTauri()) {
      setInstallMsg("请在桌面应用内一键安装");
      return;
    }
    setInstalling(true);
    setInstallMsg("正在下载 VB-CABLE 并启动官方安装程序…请留意 UAC 弹窗");
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

  async function generateReport() {
    if (!isTauri()) {
      setReportMsg("浏览器预览：请在桌面应用内生成诊断报告");
      return;
    }
    setReportMsg("正在生成诊断报告…");
    try {
      const path = await invoke<string>("generate_diagnostic_report");
      setReportMsg(`诊断报告已保存：${path}`);
    } catch (err) {
      setReportMsg(`生成失败：${err}`);
    }
  }

  async function openReportDir() {
    if (!isTauri()) {
      return;
    }
    try {
      await invoke("open_log_dir");
    } catch (err) {
      setReportMsg(`打开报告目录失败：${err}`);
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
        deviceName: OUTPUT_DEVICE,
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

  const testTotal = selfTests?.length ?? 0;
  const testPass = selfTests?.filter((t) => t.status === "pass").length ?? 0;
  const testFail = selfTests?.filter((t) => t.status === "fail").length ?? 0;

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>诊断</h1>
          <p>检查虚拟声卡、虚拟键盘与语音链路，出问题时在这里生成报告。</p>
        </div>
      </div>

      <section className="card diag-card">
        <div className="diag-head">
          <div className="diag-head-text">
            <div className="diag-title">系统状态与自检</div>
            <div className="diag-status">
              {status}
              {selfTests && (
                <span className="diag-status-meta">
                  共 {testTotal} 项 · 通过 {testPass}
                  {testFail > 0 ? ` · 需处理 ${testFail}` : ""}
                </span>
              )}
            </div>
          </div>
          <div className="diag-head-actions">
            <button className="btn primary" onClick={runSelfTest}>
              <IconSearch size={14} />
              运行全自检
            </button>
            <button className="btn" onClick={generateReport}>
              生成报告
            </button>
          </div>
        </div>

        <div className="diag-rows">
          <div className="diag-row">
            <span className="diag-row-label">虚拟声卡（VB-CABLE）</span>
            <span className="diag-row-value">
              <span className={`pill ${data.has_vb_cable ? "ok" : "warn"}`}>
                <span className="pill-dot" />
                {data.has_vb_cable ? "正常" : "未就绪"}
              </span>
              <span className={`pill ${data.cable_input_present ? "ok" : "warn"}`}>
                <span className="pill-dot" />
                CABLE 输入
              </span>
              <span className={`pill ${data.cable_output_present ? "ok" : "warn"}`}>
                <span className="pill-dot" />
                CABLE 输出
              </span>
              {checked && !data.has_vb_cable && (
                <button
                  className="btn small"
                  onClick={installVbCable}
                  disabled={installing || !isTauri()}
                >
                  {installing ? "正在下载…" : "下载并安装"}
                </button>
              )}
            </span>
          </div>

          {selfTests ? (
            selfTests.map((t) => (
              <div className="diag-row" key={t.name}>
                <span className="diag-row-label">{t.name}</span>
                <span className="diag-row-value">
                  {t.detail && <span className="diag-row-detail">{t.detail}</span>}
                  <span
                    className={`pill ${
                      t.status === "pass" ? "ok" : t.status === "fail" ? "err" : "warn"
                    }`}
                  >
                    {t.status === "pass" ? "通过" : t.status === "fail" ? "失败" : "跳过"}
                  </span>
                </span>
              </div>
            ))
          ) : (
            <div className="diag-empty">
              还没跑过自检。点右上角「运行全自检」，会检查音频端点、虚拟声卡、虚拟键盘、
              语音链路等 10 项。
            </div>
          )}
        </div>

        {checked && !data.has_vb_cable && (
          <div className="diag-note">
            装好 VB-CABLE 后需要重启电脑，回来再点「运行全自检」。
          </div>
        )}
        {installMsg && <div className="diag-note">{installMsg}</div>}
        {vhidMsg && <div className="diag-note">{vhidMsg}</div>}
        {reportMsg && <div className="diag-note">{reportMsg}</div>}

        <div className="diag-tools">
          <button className="btn" onClick={openReportDir} disabled={!isTauri()}>
            打开报告目录
          </button>
          <button className="btn" onClick={loopTone} disabled={looping}>
            {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
          </button>
          <button className="btn" onClick={toggleQuickMenu}>
            打开 / 关闭快捷菜单
          </button>
          <button className="btn" onClick={installVhidDriver} disabled={vhidBusy || !isTauri()}>
            {vhidBusy ? "正在安装…" : "安装 / 修复虚拟键盘驱动"}
          </button>
        </div>
      </section>

      <section className="card diag-card">
        <div className="diag-head">
          <div className="diag-head-text">
            <div className="diag-title">应用专属映射</div>
            <div className="diag-status">
              当前{foreground?.profile ? `命中「${foreground.profile}」` : "使用全局映射"}
              <span className="diag-status-meta">
                已加载 {foreground?.profile_count ?? 0} 份配置
              </span>
            </div>
          </div>
          <div className="diag-head-actions">
            <button className="btn" onClick={refreshForeground}>
              刷新
            </button>
            <button className="btn" onClick={reloadProfiles}>
              重载配置
            </button>
          </div>
        </div>

        <div className="diag-rows">
          <div className="diag-row">
            <span className="diag-row-label">当前前台进程</span>
            <span className="diag-row-value diag-mono">
              {foreground?.process ?? "读不到前台窗口"}
            </span>
          </div>
          <div className="diag-row">
            <span className="diag-row-label">当前窗口标题</span>
            <span className="diag-row-value diag-ellipsis" title={foreground?.title ?? ""}>
              {foreground?.title ?? "—"}
            </span>
          </div>
          <div className="diag-row">
            <span className="diag-row-label">命中的应用配置</span>
            <span className="diag-row-value">
              {foreground?.profile ?? "未命中"}
            </span>
          </div>
          <div className="diag-row">
            <span className="diag-row-label">覆盖的按键</span>
            <span className="diag-row-value diag-ellipsis">
              {foreground?.overridden_buttons.length
                ? foreground.overridden_buttons.join("、")
                : "—"}
            </span>
          </div>
        </div>

        {profileMsg && <div className="diag-note">{profileMsg}</div>}
        <p className="diag-help">
          切到目标应用后点「刷新」：这里的进程名就是配置里的 <code>process</code>；
          网页应用或跑在 WSL 里的服务认不出进程，改用 <code>window_title_contains</code> 匹配标题。
        </p>
      </section>

      <div className="section-label">按键测试</div>
      <RemoteKeyTester />
    </div>
  );
}
