import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

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

const BRIEFS = [
  { label: "蓝牙", value: "待连接", tone: "warn" },
  { label: "设备", value: "RC003 / 2 Pro", tone: "ok" },
  { label: "ATVV 语音", value: "未启用", tone: "warn" },
];

export function ConnectionPage() {
  const [scanResult, setScanResult] = useState("未开始扫描");
  const [scanning, setScanning] = useState(false);
  const [connectResult, setConnectResult] = useState("");
  const [connecting, setConnecting] = useState(false);

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
    setConnectResult("正在连接并枚举 GATT（UNCACHED）…");
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      setConnectResult(
        `已连接 ${result.device.name}；ATVV：音频=${result.endpoints.audio ? "有" : "无"}，控制=${result.endpoints.control ? "有" : "无"}（已记住设备）`
      );
      try {
        await invoke("save_selected_device", { deviceId: result.device.id });
      } catch {
        setConnectResult(prev => `${prev}（保存设备失败）`);
      }
    } catch (err) {
      setConnectResult(`连接失败：${err}`);
    } finally {
      setConnecting(false);
    }
  }

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
        <span className="badge badge-warn">未连接</span>
      </section>

      <section className="card">
        <div className="card-title">状态概览</div>
        <div className="brief-grid">
          {BRIEFS.map((b) => (
            <div key={b.label} className={`brief ${b.tone}`}>
              <div className="brief-value">{b.value}</div>
              <div className="brief-label">{b.label}</div>
            </div>
          ))}
        </div>
      </section>

      <section className="card">
        <div className="card-title">扫描遥控器</div>
        <div className="actions">
          <button className="btn primary" onClick={scan} disabled={!isTauri() || scanning}>
            {scanning ? "扫描中…" : "扫描遥控器"}
          </button>
          <button
            className="btn"
            onClick={connect}
            disabled={!isTauri() || connecting}
          >
            {connecting ? "连接中…" : "连接并发现 ATVV"}
          </button>
        </div>
        <p className="hint">{scanResult}</p>
        <p className="hint">{connectResult}</p>
      </section>
    </div>
  );
}
