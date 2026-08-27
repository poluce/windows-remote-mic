import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

type BridgeStatus = "idle" | "running" | "failed";
type TapStatus = "idle" | "attached" | "pending" | "unavailable";

function mapTapStatus(msg: string): TapStatus {
  if (msg.includes("已附着")) return "attached";
  if (msg.includes("缺少") || msg.includes("拒绝") || msg.includes("失败")) return "unavailable";
  if (msg) return "pending";
  return "idle";
}

function tapStatusLabel(status: TapStatus, connected: boolean): string {
  switch (status) {
    case "attached":
      return "已附着";
    case "pending":
      return "处理中";
    case "unavailable":
      return "未启用";
    case "idle":
      return connected ? "等待语音通道" : "未启用";
  }
}

function tapStatusTone(status: TapStatus): string {
  return status === "attached" ? "ok" : "warn";
}

function endpointsLabel(endpoints: AtvvEndpoints | null): string {
  if (!endpoints) return "未知";
  const audio = endpoints.audio ? "音频 ✓" : "音频 ✗";
  const control = endpoints.control ? "控制 ✓" : "控制 ✗";
  return `${audio} / ${control}`;
}

export function ConnectionPage() {
  const [connected, setConnected] = useState(false);
  const [bridgeStatus, setBridgeStatus] = useState<BridgeStatus>("idle");
  const [tapStatus, setTapStatus] = useState<TapStatus>("idle");
  const [endpoints, setEndpoints] = useState<AtvvEndpoints | null>(null);
  const [scanning, setScanning] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [feedback, setFeedback] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    let unlistenTap: (() => void) | undefined;
    let unlistenBle: (() => void) | undefined;
    listen<string>("hid-tap-status", (event) => {
      setTapStatus(mapTapStatus(event.payload));
    }).then((fn) => {
      unlistenTap = fn;
    });
    listen<boolean>("ble-connection-status", (event) => {
      setConnected(event.payload);
      if (!event.payload) {
        setBridgeStatus("idle");
      }
    }).then((fn) => {
      unlistenBle = fn;
    });
    return () => {
      unlistenTap?.();
      unlistenBle?.();
    };
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

  async function connect() {
    if (!isTauri()) {
      setFeedback("请在桌面应用内连接");
      return;
    }
    setConnecting(true);
    setFeedback("正在连接并枚举 GATT 特征…");
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      setConnected(true);
      setEndpoints(result.endpoints);
      setFeedback("连接成功");
      try {
        await invoke("save_selected_device", { deviceId: result.device.id });
      } catch {
        // 忽略保存错误
      }

      // 连接成功后，自动启动后台语音桥
      try {
        const bridgeRes = await invoke<string>("start_voice_bridge", {
          deviceId: result.device.id,
          outputDevice: "CABLE Input",
        });
        setBridgeStatus("running");
        setFeedback(bridgeRes);
      } catch (bridgeErr) {
        setBridgeStatus("failed");
        setFeedback(`连接成功，但语音桥启动失败：${bridgeErr}`);
      }
    } catch (err) {
      setConnected(false);
      setBridgeStatus("idle");
      setFeedback(`连接失败：${err}`);
    } finally {
      setConnecting(false);
    }
  }

  const briefs = [
    {
      label: "ATVV 语音桥",
      value: bridgeStatus === "running" ? "运行中" : bridgeStatus === "failed" ? "启动失败" : "未启用",
      tone: bridgeStatus === "running" ? "ok" : "warn",
    },
    {
      label: "返回/音量旁路",
      value: tapStatusLabel(tapStatus, connected),
      tone: tapStatusTone(tapStatus),
    },
    {
      label: "ATVV 端点",
      value: endpointsLabel(endpoints),
      tone: endpoints?.audio && endpoints.control ? "ok" : "warn",
    },
  ];

  return (
    <div className="page">
      <section className="card device-card">
        <div className="device-info">
          <span className="device-icon">📡</span>
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
            <button className="btn" onClick={scan} disabled={!isTauri() || scanning}>
              {scanning ? "扫描中…" : "扫描"}
            </button>
            <button
              className="btn primary"
              onClick={connect}
              disabled={!isTauri() || connecting}
            >
              {connecting ? "连接中…" : "连接"}
            </button>
          </div>
        </div>
        {feedback && <p className="hint device-feedback">{feedback}</p>}
      </section>

      <section className="card">
        <div className="card-title">状态概览</div>
        <div className="brief-grid">
          {briefs.map((b) => (
            <div key={b.label} className={`brief ${b.tone}`}>
              <div className="brief-value">{b.value}</div>
              <div className="brief-label">{b.label}</div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
