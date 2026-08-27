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

export function ConnectionPage() {
  const [scanResult, setScanResult] = useState("未开始扫描");
  const [scanning, setScanning] = useState(false);
  const [connectResult, setConnectResult] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [connected, setConnected] = useState(false);
  const [bridgeRunning, setBridgeRunning] = useState(false);
  const [tapStatus, setTapStatus] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    listen<string>("hid-tap-status", (event) => {
      setTapStatus(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<{ selected_device_id?: string }>("get_persisted_settings")
      .then((cfg) => {
        if (cfg.selected_device_id) {
          setScanResult(`已记忆设备 ID: ${cfg.selected_device_id}`);
        }
      })
      .catch(() => {});
  }, []);

  async function scan() {
    if (!isTauri()) {
      setScanResult("浏览器预览：请在桌面应用内扫描");
      return;
    }
    setScanning(true);
    setScanResult("正在扫描蓝牙…（请确认遥控器已在 Windows 蓝牙中配对）");
    try {
      const device = await invoke<Rc003Device>("scan_for_rc003");
      setScanResult(`已找到：${device.name}`);
    } catch (err) {
      setScanResult(`未找到：${err}`);
    } finally {
      setScanning(false);
    }
  }

  async function connect() {
    if (!isTauri()) {
      setConnectResult("浏览器预览：请在桌面应用内连接");
      return;
    }
    setConnecting(true);
    setConnectResult("正在连接并枚举 GATT 特征…（如长时间无响应，请按一下遥控器按键以唤醒蓝牙）");
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      setConnected(true);
      setConnectResult(
        `已连接 ${result.device.name}；ATVV：音频=${result.endpoints.audio ? "有" : "无"}，控制=${result.endpoints.control ? "有" : "无"}`
      );
      try {
        await invoke("save_selected_device", { deviceId: result.device.id });
      } catch {
        // ignore save error
      }

      // 连接成功后，自动启动后台语音桥
      try {
        const bridgeRes = await invoke<string>("start_voice_bridge", {
          deviceId: result.device.id,
          outputDevice: "CABLE Input",
        });
        setBridgeRunning(true);
        setConnectResult((prev) => `${prev} · ${bridgeRes}`);
      } catch (bridgeErr) {
        setConnectResult((prev) => `${prev}（启动语音桥失败：${bridgeErr}）`);
      }
    } catch (err) {
      setConnected(false);
      setBridgeRunning(false);
      setConnectResult(`连接失败：${err}`);
    } finally {
      setConnecting(false);
    }
  }

  const briefs = [
    {
      label: "蓝牙",
      value: connected ? "已连接" : "待连接",
      tone: connected ? "ok" : "warn",
    },
    { label: "设备", value: "RC003 / 2 Pro", tone: "ok" },
    {
      label: "ATVV 语音桥",
      value: bridgeRunning ? "运行中" : "未启用",
      tone: bridgeRunning ? "ok" : "warn",
    },
    {
      label: "返回/音量旁路",
      value: tapStatus.includes("已附着")
        ? "已附着"
        : tapStatus.includes("缺少") || tapStatus.includes("拒绝") || tapStatus.includes("失败")
          ? "未启用"
          : tapStatus
            ? "处理中"
            : connected
              ? "等待语音通道"
              : "未启用",
      tone: tapStatus.includes("已附着") ? "ok" : "warn",
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
        <span className={`badge ${connected ? "badge-ok" : "badge-warn"}`}>
          {connected ? "已连接" : "未连接"}
        </span>
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

      <section className="card">
        <div className="card-title">连接遥控器</div>
        <div className="actions">
          <button className="btn" onClick={scan} disabled={!isTauri() || scanning}>
            {scanning ? "扫描中…" : "扫描遥控器"}
          </button>
          <button
            className="btn primary"
            onClick={connect}
            disabled={!isTauri() || connecting}
          >
            {connecting ? "连接中…" : "连接并启动语音桥"}
          </button>
        </div>
        {scanResult && <p className="hint">{scanResult}</p>}
        {connectResult && <p className="hint">{connectResult}</p>}
        {tapStatus && <p className="hint">{tapStatus}</p>}
      </section>
    </div>
  );
}
