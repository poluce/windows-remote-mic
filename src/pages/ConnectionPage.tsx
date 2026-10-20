import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  bridgeStateLabel,
  markConnected,
  markDisconnected,
  useEffectiveConnected,
  tapStatusLabel,
  useRuntimeStatus,
} from "../store/runtimeStatus";

/** `stop_voice_bridge` 的返回：是否真的停下 + 提示文案。 */
type BridgeStopResult = {
  stopped: boolean;
  message: string;
};
import { IconBluetooth } from "../components/icons";
import { OUTPUT_DEVICE } from "../constants";

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
  /** 真实 BLE 链路状态（GATT 缓存发现成功 ≠ 链路已连接）。 */
  link_connected: boolean;
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
    tapStatus,
    tapMessage,
    endpointsReady,
    bridgeState,
    bridgeAttempt,
    bridgeFailures,
    bridgeNextRetrySecs,
    bridgeLastError,
  } = runtime;
  // 连接状态派生视图：等待态一定未连接（页面与 Sidebar 共用同一判断）。
  const effectiveConnected = useEffectiveConnected();
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
      markConnected(endpointsReady, result.link_connected);
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
          outputDevice: OUTPUT_DEVICE,
        });
      } catch (bridgeErr) {
        setFeedback(`语音桥启动失败：${bridgeErr}`);
      }
    } catch (err) {
      // 设备当前不可达（休眠 / 被另一台电脑占用 / 蓝牙未开）：
      // 只要本机还留着配对信息，就进入「等待遥控器上线」模式——
      // 监督线程只监听链路事件，系统连上后会自动恢复语音，不再定时盲试。
      const savedId = await invoke<string | null>("get_saved_device_id").catch(
        () => null,
      );
      if (savedId) {
        try {
          await invoke<string>("start_voice_bridge", {
            deviceId: savedId,
            outputDevice: OUTPUT_DEVICE,
          });
          setFeedback(
            options?.auto
              ? "遥控器当前不在线，已进入等待（系统连上后自动恢复）"
              : "遥控器当前不在线，已进入等待：请唤醒遥控器，系统连接后会自动恢复语音",
          );
          return;
        } catch (bridgeErr) {
          setFeedback(`语音桥启动失败：${bridgeErr}`);
          return;
        }
      }
      setFeedback(
        options?.auto
          ? "自动连接未成功，请唤醒遥控器后点「连接」重试"
          : `连接失败：${err}`,
      );
    } finally {
      setConnecting(false);
    }
  }

  /** 失败状态下重试：先停旧监督线程，再重新启动。 */
  async function retryBridge() {
    const savedId = await invoke<string | null>("get_saved_device_id").catch(
      () => null,
    );
    if (!savedId) {
      setFeedback("没有已保存的遥控器，请先重新配对后再连接");
      return;
    }
    setConnecting(true);
    setFeedback("正在重试连接…");
    try {
      const message = await invoke<string>("retry_voice_bridge", {
        deviceId: savedId,
        outputDevice: OUTPUT_DEVICE,
      });
      setFeedback(message);
    } catch (err) {
      setFeedback(`重试失败：${err}`);
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
    if (autoConnectTried.current || !isTauri() || effectiveConnected || connecting) {
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
      : bridgeState === "failed"
        ? "err"
        : "warn";
  const bridgeLabel =
    bridgeState === "waiting"
      ? "等待遥控器上线"
      : bridgeStateLabel(bridgeState, bridgeAttempt);

  // 遥控器语音固件挂起的专属提示（后端在连续零数据会话后给出该文案）。
  const silentSessionHint = bridgeLastError.includes("语音固件可能挂起");
  // 监督线程活着（等待链路/建会话/运行/重试）→ 按钮显示「停止」而不是「连接」。
  const bridgeActive =
    bridgeState === "starting" ||
    bridgeState === "running" ||
    bridgeState === "retrying" ||
    bridgeState === "waiting";
  const showBridgeHint =
    (bridgeState === "retrying" ||
      bridgeState === "failed" ||
      bridgeState === "starting" ||
      bridgeState === "waiting") &&
    !silentSessionHint;

  const briefs: { key: string; label: string; tone: string; title?: string }[] = [
    {
      key: "bridge",
      label: bridgeLabel,
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
      // 语音桥运行中必然已经发现端点；即使快照里的标志因事件时序暂时滞后，
      // 也不能在这里显示「未就绪」，避免和「ATVV 语音桥（运行中）」矛盾。
      tone: endpointsReady || bridgeState === "running" ? "ok" : "warn",
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
            <span
              className={`badge ${effectiveConnected ? "badge-ok" : "badge-warn"}`}
            >
              {effectiveConnected ? "已连接" : "未连接"}
            </span>
            <div className="actions">
              <button
                className="btn"
                onClick={scan}
                disabled={!isTauri() || scanning || effectiveConnected}
              >
                {scanning ? "扫描中…" : "扫描"}
              </button>
              {effectiveConnected || bridgeActive ? (
                <button
                  className="btn"
                  onClick={disconnect}
                  disabled={!isTauri() || disconnecting}
                >
                  {disconnecting
                    ? "停止中…"
                    : effectiveConnected
                      ? "断开"
                      : "停止等待"}
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
        {silentSessionHint && (
          <div className="bridge-hint err">
            <span className="bridge-hint-title">遥控器语音服务可能已挂起</span>
            <span className="bridge-hint-body">
              蓝牙链路正常，但连续多次会话都收不到遥控器的语音数据。请按顺序操作：
              <br />
              1. 取下遥控器电池，等待约 10 秒后装回（或长按电源键重启遥控器）；
              <br />
              2. 回到本页点「断开」，再点「连接」；
              <br />
              3. 长按麦克风键说话，确认语音条有反应。
            </span>
          </div>
        )}
        {showBridgeHint && (
          <div
            className={`bridge-hint ${bridgeState === "failed" ? "err" : "warn"}`}
          >
            <span className="bridge-hint-title">
              {bridgeState === "failed"
                ? "语音桥连接失败"
                : bridgeState === "waiting"
                  ? "等待遥控器上线"
                  : bridgeState === "starting"
                    ? "正在建立语音链路…"
                    : "语音桥正在重试"}
            </span>
            <span className="bridge-hint-body">
              {bridgeState === "waiting"
                ? bridgeLastError
                  ? `${bridgeLastError}（系统连上后会自动恢复；若已在系统里删除配对，请重新配对）`
                  : "遥控器当前不在本机（可能休眠，或正在被另一台电脑使用）。系统连上它之后会自动恢复语音，无需重复点击；点「停止等待」可退出。"
                : bridgeState === "starting"
                  ? "正在建立蓝牙语音链路，请稍候…"
                  : bridgeLastError
                    ? `最近错误：${bridgeLastError}`
                    : "语音会话中断，正在有限次重试"}
              {bridgeFailures > 0 && ` · 连续失败 ${bridgeFailures} 次`}
              {bridgeState === "retrying" &&
                bridgeNextRetrySecs > 0 &&
                ` · 约 ${bridgeNextRetrySecs} 秒后重试`}
            </span>
            {bridgeState === "failed" && (
              <div className="bridge-hint-actions">
                <button
                  className="btn"
                  onClick={retryBridge}
                  disabled={!isTauri() || connecting}
                >
                  {connecting ? "重试中…" : "重试连接"}
                </button>
              </div>
            )}
          </div>
        )}
        {bridgeState === "stopped" && !effectiveConnected && (
          <div className="bridge-hint warn">
            <span className="bridge-hint-title">语音桥已停止</span>
            <span className="bridge-hint-body">
              遥控器按键接管已暂停：现在按遥控器按键不会触发任何映射（也不会再唤起输入法）。
              点上方「连接」可恢复语音与按键映射；若要彻底断开遥控器，请在 Windows 蓝牙设置中删除设备。
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
                    : bridgeState === "retrying" ||
                        bridgeState === "starting" ||
                        bridgeState === "waiting"
                      ? "warn"
                      : "preview"
              }`}
            />
            <span className="bridge-inline-text">
              {bridgeLabel}
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
