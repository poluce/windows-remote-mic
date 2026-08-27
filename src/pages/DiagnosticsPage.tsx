import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { RemoteKeyTester } from "../components/RemoteKeyTester";

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

type LogInfo = {
  path: string;
  file_size: number;
  debug_enabled: boolean;
  files: {
    name: string;
    path: string;
    size: number;
    modified: number | null;
  }[];
};

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

export function DiagnosticsPage() {
  const [data, setData] = useState<Diagnostics>(EMPTY);
  const [status, setStatus] = useState("请在桌面应用内运行检查");
  const [checked, setChecked] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installMsg, setInstallMsg] = useState("");
  const [looping, setLooping] = useState(false);
  const [selfTests, setSelfTests] = useState<SelfTestItem[] | null>(null);

  const [logInfo, setLogInfo] = useState<LogInfo | null>(null);
  const [logContent, setLogContent] = useState("");
  const [logLoading, setLogLoading] = useState(false);
  const [logMsg, setLogMsg] = useState("");

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

  async function refreshLogInfo() {
    if (!isTauri()) return;
    try {
      setLogInfo(await invoke<LogInfo>("get_log_info"));
    } catch (err) {
      setLogMsg(`读取日志信息失败: ${err}`);
    }
  }

  async function loadLogTail() {
    if (!isTauri()) {
      setLogContent("请在桌面应用内查看日志");
      return;
    }
    setLogLoading(true);
    setLogMsg("");
    try {
      const text = await invoke<string>("read_log_tail", { maxBytes: 64 * 1024 });
      setLogContent(text || "（日志为空）");
      await refreshLogInfo();
    } catch (err) {
      setLogMsg(`读取日志失败: ${err}`);
    } finally {
      setLogLoading(false);
    }
  }

  async function clearLogFile() {
    if (!isTauri()) return;
    if (!window.confirm("确定清空当前日志文件？此操作不可撤销。")) {
      return;
    }
    setLogMsg("");
    try {
      await invoke("clear_log");
      setLogContent("（日志已清空）");
      setLogMsg("日志已清空");
      await refreshLogInfo();
    } catch (err) {
      setLogMsg(`清空日志失败: ${err}`);
    }
  }

  async function openLogDir() {
    if (!isTauri()) {
      setLogMsg("请在桌面应用内打开日志目录");
      return;
    }
    try {
      await invoke("open_log_dir");
      setLogMsg("已在文件管理器中打开日志目录");
    } catch (err) {
      setLogMsg(`打开日志目录失败: ${err}`);
    }
  }

  async function toggleDebugLogging() {
    if (!isTauri()) return;
    const enabled = !(logInfo?.debug_enabled ?? false);
    try {
      const result = await invoke<boolean>("set_debug_logging", { enabled });
      setLogInfo((prev) => (prev ? { ...prev, debug_enabled: result } : prev));
      setLogMsg(result ? "已开启 DEBUG 详细日志" : "已关闭 DEBUG 详细日志");
    } catch (err) {
      setLogMsg(`切换 DEBUG 日志失败: ${err}`);
    }
  }

  useEffect(() => {
    if (isTauri()) {
      refreshLogInfo();
      loadLogTail();
    }
  }, []);

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
        {checked && !data.has_vb_cable && (
          <div className="actions">
            <button className="btn primary" onClick={installVbCable} disabled={installing || !isTauri()}>
              {installing ? "正在安装…" : "一键安装 VB-CABLE"}
            </button>
            {!isTauri() && <span className="hint">请在桌面应用内安装</span>}
          </div>
        )}
        {installMsg && <p className="hint">{installMsg}</p>}
      </section>

      <section className="card">
        <div className="card-title">诊断与自检操作</div>
        <div className="actions">
          <button className="btn primary" onClick={runSelfTest}>
            🔍 运行系统全自检
          </button>
          <button className="btn" onClick={loopTone} disabled={looping}>
            {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
          </button>
          <button className="btn" onClick={toggleQuickMenu}>
            打开/关闭快捷菜单
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

      <section className="card">
        <div className="card-title">日志</div>
        <div className="log-actions">
          <button className="btn" onClick={loadLogTail} disabled={logLoading}>
            {logLoading ? "读取中…" : "刷新日志"}
          </button>
          <button className="btn" onClick={clearLogFile}>清空日志</button>
          <button className="btn" onClick={openLogDir}>打开日志目录</button>
          <button className="btn" onClick={toggleDebugLogging}>
            {logInfo?.debug_enabled ? "关闭 DEBUG" : "开启 DEBUG"}
          </button>
        </div>
        {logInfo && (
          <p className="hint">
            当前日志：{logInfo.path}（{formatSize(logInfo.file_size)}）
            {logInfo.files.length > 1 && `，已保留 ${logInfo.files.length - 1} 个轮转文件`}
          </p>
        )}
        {logMsg && <p className="hint">{logMsg}</p>}
        <div className="log-preview">{logContent || "（暂无日志内容）"}</div>
      </section>

      <RemoteKeyTester />
    </div>
  );
}
