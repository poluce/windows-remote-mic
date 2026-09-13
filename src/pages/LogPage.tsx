import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

export function LogPage() {
  const [logInfo, setLogInfo] = useState<LogInfo | null>(null);
  const [logContent, setLogContent] = useState("");
  const [logLoading, setLogLoading] = useState(false);
  const [logMsg, setLogMsg] = useState("");
  const logContentRef = useRef("");
  const logViewerRef = useRef<HTMLDivElement | null>(null);
  const [liveLogCount, setLiveLogCount] = useState(0);

  async function refreshLogInfo() {
    if (!isTauri()) return;
    try {
      setLogInfo(await invoke<LogInfo>("get_log_info"));
    } catch (err) {
      setLogMsg(`读取日志信息失败: ${err}`);
    }
  }

  async function loadLogTail() {
    if (!isTauri()) return;
    try {
      const text = await invoke<string>("read_log_tail", { maxBytes: 256 * 1024 });
      const trimmed = text ? text.trimEnd() : "";
      logContentRef.current = trimmed;
      setLogContent(trimmed || "（暂无日志内容）");
      setTimeout(() => {
        if (logViewerRef.current) {
          logViewerRef.current.scrollTop = logViewerRef.current.scrollHeight;
        }
      }, 50);
    } catch (err) {
      setLogMsg(`读取日志失败: ${err}`);
    }
  }

  async function refreshLogFromFile() {
    if (!isTauri()) return;
    setLogLoading(true);
    setLogMsg("");
    try {
      await loadLogTail();
      await refreshLogInfo();
    } finally {
      setLogLoading(false);
    }
  }

  function appendLogLine(line: string) {
    const current = logContentRef.current;
    const updated = current ? `${current}\n${line}` : line;
    const lines = updated.split("\n");
    const finalLines = lines.length > 1000 ? lines.slice(lines.length - 1000) : lines;
    const nextContent = finalLines.join("\n");
    logContentRef.current = nextContent;
    setLogContent(nextContent);
    setLiveLogCount((c) => c + 1);
    if (logViewerRef.current) {
      logViewerRef.current.scrollTop = logViewerRef.current.scrollHeight;
    }
  }

  async function clearLogFile() {
    if (!isTauri()) return;
    setLogMsg("");
    try {
      await invoke("clear_log");
      logContentRef.current = "";
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
      await refreshLogInfo();
    } catch (err) {
      setLogMsg(`切换 DEBUG 日志失败: ${err}`);
    }
  }

  useEffect(() => {
    if (!isTauri()) {
      setLogContent("浏览器预览：请在桌面应用内查看运行日志");
      return;
    }
    refreshLogInfo();
    loadLogTail();

    let unlistenLine: UnlistenFn | undefined;
    let cancelled = false;

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
      unlistenLine?.();
    };
  }, []);

  return (
    <div className="page log-page">
      <div className="section-label">运行日志</div>
      <section className="card log-card">
        <div className="log-actions">
          <button className="btn" onClick={clearLogFile} disabled={!isTauri()}>
            清空日志
          </button>
          <button className="btn" onClick={openLogDir} disabled={!isTauri()}>
            打开日志目录
          </button>
          <button className="btn" onClick={toggleDebugLogging} disabled={!isTauri()}>
            {logInfo?.debug_enabled ? "关闭 DEBUG" : "开启 DEBUG"}
          </button>
          <button
            className="log-refresh-btn log-refresh-right"
            onClick={refreshLogFromFile}
            disabled={logLoading || !isTauri()}
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
        <div className="log-preview" ref={logViewerRef}>
          {logContent || "（暂无日志内容）"}
        </div>
      </section>
    </div>
  );
}
