import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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

// 诊断页日志过滤：屏蔽键盘/钩子高频刷屏行，保留连接、旁路、错误等关键日志。
function isNoisyLogLine(line: string): boolean {
  return (
    line.includes("[raw_input] 键盘事件") ||
    line.includes("[hook] 低层键盘事件")
  );
}

function filterLogText(text: string): string {
  return text
    .split("\n")
    .filter((line) => line.trim() !== "" && !isNoisyLogLine(line))
    .join("\n");
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
  const logContentRef = useRef("");
  const historyLoadedRef = useRef(false);
  const pendingLinesRef = useRef<string[]>([]);
  const [liveLogCount, setLiveLogCount] = useState(0);

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

  // 手动刷新按钮：主动读取一次最新文件尾部（用户主动操作，非轮询）。
  async function refreshLogFromFile() {
    if (!isTauri()) return;
    setLogLoading(true);
    setLogMsg("");
    try {
      const text = await invoke<string>("read_log_tail", { maxBytes: 256 * 1024 });
      applyLogHistory(text);
      await refreshLogInfo();
    } catch (err) {
      setLogMsg(`读取日志失败: ${err}`);
    } finally {
      setLogLoading(false);
    }
  }

  // 前端不再主动读日志文件；历史由后端通过 log-history 事件推送。
  function applyLogHistory(history: string) {
    let initial = filterLogText(history || "");
    // 合并历史推送前到达的实时行（事件时序竞态兜底）。
    const pending = pendingLinesRef.current.filter((l) => !isNoisyLogLine(l));
    pendingLinesRef.current = [];
    if (pending.length > 0) {
      initial += (initial ? "\n" : "") + pending.join("\n");
    }
    setLogContent(initial || "（日志为空）");
    logContentRef.current = initial;
    historyLoadedRef.current = true;
  }

  function appendLogLine(line: string) {
    if (isNoisyLogLine(line)) return;
    if (!historyLoadedRef.current) {
      // 历史还没到达时先缓存，等 log-history 到达后统一合并。
      pendingLinesRef.current.push(line);
      return;
    }
    logContentRef.current += line + "\n";
    // 只保留最近约 1000 行，避免无限增长。
    const lines = logContentRef.current.split("\n");
    if (lines.length > 1000) {
      logContentRef.current = lines.slice(lines.length - 1000).join("\n");
    }
    setLogContent(logContentRef.current);
    setLiveLogCount((c) => c + 1);
  }

  async function clearLogFile() {
    if (!isTauri()) return;
    setLogMsg("");
    try {
      await invoke("clear_log");
      setLogContent("（日志已清空）");
      logContentRef.current = "";
      // 清空后历史已经是最新（空），后续实时行应直接显示，不能缓存等待。
      historyLoadedRef.current = true;
      pendingLinesRef.current = [];
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
    if (!isTauri()) return;
    refreshLogInfo();

    let unlistenHistory: UnlistenFn | undefined;
    let unlistenLine: UnlistenFn | undefined;
    let cancelled = false;

    listen<string>("log-history", (event) => {
      if (cancelled) return;
      applyLogHistory(event.payload);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlistenHistory = fn;
      }
    });

    listen<string>("log-line", (event) => {
      if (cancelled) return;
      appendLogLine(event.payload);
    }).then((fn) => {
      if (cancelled) {
        fn();
      } else {
        unlistenLine = fn;
      }
    });

    return () => {
      cancelled = true;
      unlistenHistory?.();
      unlistenLine?.();
    };
    // applyLogHistory / appendLogLine 是稳定函数，无需加入依赖
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="page">

      <section className="card vbcable-card">
        <span className="vbcable-title">虚拟声卡（VB-CABLE）</span>
        <span className={`vbcable-status ${data.has_vb_cable ? "ok" : "warn"}`}>
          {data.has_vb_cable ? "正常" : "未就绪"}
        </span>
        <span className={`vbcable-status ${data.cable_input_present ? "ok" : "warn"}`}>
          CABLE 输入
        </span>
        <span className={`vbcable-status ${data.cable_output_present ? "ok" : "warn"}`}>
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
          🔍 运行系统全自检
        </button>
        <button className="btn" onClick={loopTone} disabled={looping}>
          {looping ? "循环播放中…" : "循环播放测试音（3 次）"}
        </button>
        <button className="btn" onClick={toggleQuickMenu}>
          打开/关闭快捷菜单
        </button>
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

      <section className="card">
        <div className="log-actions">
          <button className="btn" onClick={clearLogFile}>清空日志</button>
          <button className="btn" onClick={openLogDir}>打开日志目录</button>
          <button className="btn" onClick={toggleDebugLogging}>
            {logInfo?.debug_enabled ? "关闭 DEBUG" : "开启 DEBUG"}
          </button>
          <button
            className="log-refresh-btn log-refresh-right"
            onClick={refreshLogFromFile}
            disabled={logLoading}
            title="刷新日志"
            aria-label="刷新日志"
          >
            <svg
              viewBox="0 0 24 24"
              width="16"
              height="16"
              fill="none"
              stroke="currentColor"
              strokeWidth="2.2"
              strokeLinecap="round"
              strokeLinejoin="round"
              className={logLoading ? "spinning" : ""}
            >
              <path d="M21 12a9 9 0 1 1-2.64-6.36" />
              <polyline points="21 3 21 9 15 9" />
            </svg>
          </button>
        </div>
        {logInfo && (
          <p className="hint">
            当前日志：{logInfo.path}（{formatSize(logInfo.file_size)}）
            {logInfo.files.length > 1 && `，已保留 ${logInfo.files.length - 1} 个轮转文件`}
            <span className="hint"> ｜ 实时推送已接收 {liveLogCount} 行</span>
          </p>
        )}
        {logMsg && <p className="hint">{logMsg}</p>}
        <div className="log-preview">{logContent || "（暂无日志内容）"}</div>
      </section>

      <RemoteKeyTester />
    </div>
  );
}
