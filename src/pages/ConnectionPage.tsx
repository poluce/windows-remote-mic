import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  bridgeStateLabel,
  markConnected,
  markDisconnected,
  tapStatusLabel,
  useRuntimeStatus,
} from "../store/runtimeStatus";

/** `stop_voice_bridge` 的返回：是否真的停下 + 提示文案。 */
type BridgeStopResult = {
  stopped: boolean;
  message: string;
};
import { IconBluetooth } from "../components/icons";

type Rc003Device = {
  id: string;
  name: string;
};

type AtvvEndpoints = {
  tx?: string;
  audio?: string;
  control?: string;
};

type Rc003Connection = {
  device: Rc003Device;
  endpoints: AtvvEndpoints;
};

type VbCableStatus = {
  input: boolean;
  output: boolean;
  ready: boolean;
};

type DriverStatus = "loading" | "ready" | "missing" | "unknown";

const DRIVER_OPTIONS = [
  { value: "vb_cable", label: "VB-CABLE", disabled: false },
  { value: "voicemeeter", label: "Voicemeeter（预留）", disabled: true },
  { value: "rearoute", label: "ReaRoute（预留）", disabled: true },
] as const;

function tapStatusTone(status: string): string {
  return status === "attached" ? "ok" : "warn";
}

export function ConnectionPage() {
  const runtime = useRuntimeStatus();
  const {
    connected,
    bridgeStatus,
    tapStatus,
    tapMessage,
    endpointsReady,
    bridgeState,
    bridgeAttempt,
    bridgeFailures,
    bridgeNextRetrySecs,
    bridgeLastError,
  } = runtime;
  const [scanning, setScanning] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [feedback, setFeedback] = useState("");

  const [virtualDriver, setVirtualDriver] = useState("vb_cable");
  const [driverStatus, setDriverStatus] = useState<DriverStatus>("unknown");
  const [driverOpen, setDriverOpen] = useState(false);
  const driverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    if (!isTauri()) {
      setDriverStatus("unknown");
      return () => {
        cancelled = true;
      };
    }
    setDriverStatus("loading");
    invoke<VbCableStatus>("vb_cable_status")
      .then((s) => {
        if (!cancelled) setDriverStatus(s.ready ? "ready" : "missing");
      })
      .catch(() => {
        if (!cancelled) setDriverStatus("missing");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (driverRef.current && !driverRef.current.contains(e.target as Node)) {
        setDriverOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, []);

  async function scan() {
    if (!isTauri()) {
      setFeedback("请在桌面应用内扫描");
      return;
    }
    setScanning(true);
    setFeedback("正在扫描蓝牙…（请确认遥控器已在 Windows 蓝牙中配对）");
    try {
      const device = await invoke<Rc003Device>("scan_for_rc003");
      setFeedback(`扫描成功：${device.name}`);
    } catch (err) {
      setFeedback(`扫描失败：${err}`);
    } finally {
      setScanning(false);
    }
  }

  async function connect(options?: { auto?: boolean }) {
    if (!isTauri()) {
      setFeedback("请在桌面应用内连接");
      return;
    }
    setConnecting(true);
    setFeedback(
      options?.auto
        ? "检测到上次使用的遥控器，正在自动连接…"
        : "正在连接并枚举 GATT 特征…",
    );
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      const endpointsReady = Boolean(result.endpoints.audio && result.endpoints.control);
      markConnected(endpointsReady);
      setFeedback("");
      try {
        await invoke("save_selected_device", { deviceId: result.device.id });
      } catch {
        // 忽略保存错误
      }

      // 连接成功后，自动启动后台语音桥
      try {
        await invoke<string>("start_voice_bridge", {
          deviceId: result.device.id,
          outputDevice: "CABLE Input",
        });
      } catch (bridgeErr) {
        setFeedback(`语音桥启动失败：${bridgeErr}`);
      }
    } catch (err) {
      setFeedback(
        options?.auto
          ? "自动连接未成功，请唤醒遥控器后点「连接」重试"
          : `连接失败：${err}`,
      );
    } finally {
      setConnecting(false);
    }
  }

  // 启动时自动尝试连接「上次成功连接的设备」，省掉每次开机手动点连接。
  // 仅挂载时尝试一次；失败保持安静（手动点连接会给出明确错误）。
  // 注意：React StrictMode 会「挂载→卸载→再挂载」跑两次 effect，
  // 这里不设 cancelled 短路，否则第一次的异步请求会被取消、第二次又被
  // autoConnectTried 拦住，导致自动连接永远不生效（开发模式实测）。
  const autoConnectTried = useRef(false);
  useEffect(() => {
    if (autoConnectTried.current || !isTauri() || connected || connecting) {
      return;
    }
    autoConnectTried.current = true;
    void (async () => {
      try {
        const savedId = await invoke<string | null>("get_saved_device_id");
        void invoke("log_message", {
          message: `[auto-connect] 启动自动连接：saved_device_id=${savedId ?? "null"}`,
        }).catch(() => {});
        if (!savedId) {
          return;
        }
        await connect({ auto: true });
      } catch (err) {
        // 忽略：自动连接只是锦上添花，不打扰用户。
        void invoke("log_message", {
          message: `[auto-connect] 自动连接异常：${String(err)}`,
        }).catch(() => {});
      }
    })();
    // 只在挂载时执行一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function disconnect() {
    if (!isTauri()) {
      markDisconnected();
      return;
    }
    setDisconnecting(true);
    setFeedback("");
    try {
      const result = await invoke<BridgeStopResult>("stop_voice_bridge");
      if (result.stopped) {
        markDisconnected();
        setFeedback(result.message);
      } else {
        // 后端没真正停下：不要假装断开，否则用户会以为已经停止而实际仍在重连。
        setFeedback(result.message);
      }
    } catch (err) {
      setFeedback(`断开失败：${err}`);
    } finally {
      setDisconnecting(false);
    }
  }

  const bridgeTone =
    bridgeState === "running"
      ? "ok"
      : bridgeState === "failed" || bridgeStatus === "failed"
        ? "err"
        : "warn";
  const showBridgeHint =
    bridgeState === "retrying" || bridgeState === "failed" || bridgeState === "starting";

  const briefs: { key: string; label: string; tone: string; title?: string }[] = [
    {
      key: "bridge",
      label: bridgeStateLabel(bridgeState, bridgeAttempt),
      tone: bridgeTone,
      title: bridgeLastError || undefined,
    },
    {
      key: "tap",
      label: tapStatusLabel(tapStatus),
      tone: tapStatusTone(tapStatus),
      title: tapMessage || undefined,
    },
    {
      key: "endpoints",
      label: "ATVV 端点",
      tone: endpointsReady ? "ok" : "warn",
    },
  ];

  return (
    <div className="page conn-grid">
      <div className="conn-col">
      <div className="section-label">设备</div>
      <section className="card device-card">
        <div className="device-top">
          <div className="device-info">
            <span className="device-icon">
              <IconBluetooth size={22} />
            </span>
            <div>
              <div className="device-name">小米蓝牙遥控器 2 Pro</div>
              <div className="device-model">RC003 · VID 0x2717 · PID 0x32B8</div>
            </div>
          </div>
          <div className="device-actions">
            <span className={`badge ${connected ? "badge-ok" : "badge-warn"}`}>
              {connected ? "已连接" : "未连接"}
            </span>
            <div className="actions">
              <button
                className="btn"
                onClick={scan}
                disabled={!isTauri() || scanning || connected}
              >
                {scanning ? "扫描中…" : "扫描"}
              </button>
              {connected ? (
                <button
                  className="btn"
                  onClick={disconnect}
                  disabled={!isTauri() || disconnecting}
                >
                  {disconnecting ? "断开中…" : "断开"}
                </button>
              ) : (
                <button
                  className="btn primary"
                  onClick={() => connect()}
                  disabled={!isTauri() || connecting}
                >
                  {connecting ? "连接中…" : "连接"}
                </button>
              )}
            </div>
          </div>
        </div>
        {feedback && <p className="hint device-feedback">{feedback}</p>}
        <div className="device-status">
          {briefs.map((b) => (
            <span
              key={b.key}
              className={`device-status-pill ${b.tone}`}
              title={b.title}
            >
              <span className="pill-dot" />
              {b.label}
            </span>
          ))}
        </div>
        {showBridgeHint && (
          <div className={`bridge-hint ${bridgeState === "failed" ? "err" : "warn"}`}>
            <span className="bridge-hint-title">
              {bridgeState === "failed"
                ? "语音桥连续重连失败"
                : bridgeState === "starting"
                  ? "正在启动语音桥…"
                  : "语音桥正在自动重连"}
            </span>
            <span className="bridge-hint-body">
              {bridgeState === "starting"
                ? "正在建立蓝牙语音链路，请稍候…"
                : bridgeLastError
                  ? `最近错误：${bridgeLastError}`
                  : "等待遥控器恢复连接（可尝试唤醒遥控器或重新配对）"}
              {bridgeFailures > 0 && ` · 连续失败 ${bridgeFailures} 次`}
              {bridgeState === "retrying" &&
                bridgeNextRetrySecs > 0 &&
                ` · 约 ${bridgeNextRetrySecs} 秒后重试`}
            </span>
          </div>
        )}
      </section>
      </div>

      <div className="conn-col">
      <div className="section-label">语音链路</div>
      <section className="card form-card">
        <div className="form-row">
          <span className="form-label">虚拟声卡</span>
          <div className="status-select form-control" ref={driverRef}>
            <button
              type="button"
              className={`status-select-trigger${driverOpen ? " open" : ""}`}
              onClick={() => setDriverOpen((open) => !open)}
            >
              <span className={`status-dot ${driverStatus}`} />
              <span>
                {virtualDriver === "vb_cable"
                  ? "VB-CABLE"
                  : virtualDriver === "voicemeeter"
                    ? "Voicemeeter（预留）"
                    : "ReaRoute（预留）"}
              </span>
              <span className="status-select-caret">▾</span>
            </button>
            {driverOpen && (
              <div className="status-select-menu">
                {DRIVER_OPTIONS.map((driver) => (
                  <button
                    type="button"
                    key={driver.value}
                    className="status-option"
                    disabled={driver.disabled}
                    onClick={() => {
                      setVirtualDriver(driver.value);
                      setDriverOpen(false);
                    }}
                  >
                    <span
                      className={`status-dot ${
                        driver.value === "vb_cable" ? driverStatus : "preview"
                      }`}
                    />
                    <span>{driver.label}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
        <div className="form-row">
          <span className="form-label">语音桥</span>
          <div className="form-control bridge-inline">
            <span
              className={`status-dot ${
                bridgeState === "running"
                  ? "ready"
                  : bridgeState === "failed"
                    ? "missing"
                    : bridgeState === "retrying" || bridgeState === "starting"
                      ? "warn"
                      : "preview"
              }`}
            />
            <span className="bridge-inline-text">
              {bridgeStateLabel(bridgeState, bridgeAttempt)}
              {bridgeState === "retrying" && bridgeNextRetrySecs > 0
                ? ` · 约 ${bridgeNextRetrySecs} 秒后重试`
                : ""}
              {bridgeState === "failed" && bridgeFailures > 0
                ? ` · 已连续失败 ${bridgeFailures} 次`
                : ""}
            </span>
          </div>
        </div>
        {bridgeLastError && (
          <p className="hint bridge-last-error">最近错误：{bridgeLastError}</p>
        )}
      </section>
      </div>
    </div>
  );
}
