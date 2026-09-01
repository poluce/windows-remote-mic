import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  markConnected,
  tapStatusLabel,
  useRuntimeStatus,
} from "../store/runtimeStatus";

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

const TARGET_OPTIONS = [
  { value: "windows_voice", label: "Windows 语音键入（Win + H）", status: "ready" },
  { value: "ime_wechat", label: "微信输入法（预留）", status: "preview" },
  { value: "ime_doubao", label: "豆包输入法（预留）", status: "preview" },
  { value: "ime_sogou", label: "搜狗输入法（预留）", status: "preview" },
] as const;

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
  const { connected, bridgeStatus, tapStatus, tapMessage, endpointsReady } = runtime;
  const [scanning, setScanning] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [feedback, setFeedback] = useState("");

  const [voiceTarget, setVoiceTarget] = useState("windows_voice");
  const [virtualDriver, setVirtualDriver] = useState("vb_cable");
  const [selected] = useState("CABLE 输入（VB-CABLE）");
  const [simResult, setSimResult] = useState("");
  const [driverStatus, setDriverStatus] = useState<DriverStatus>("unknown");
  const [driverOpen, setDriverOpen] = useState(false);
  const [targetOpen, setTargetOpen] = useState(false);
  const [eatEnabled, setEatEnabled] = useState<boolean | null>(null);
  const [eatBusy, setEatBusy] = useState(false);
  const driverRef = useRef<HTMLDivElement>(null);
  const targetRef = useRef<HTMLDivElement>(null);
  const simInputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (!isTauri()) {
      setEatEnabled(true);
      return;
    }
    invoke<boolean>("get_hid_tap_eat")
      .then(setEatEnabled)
      .catch(() => setEatEnabled(true));
  }, []);

  async function toggleEat() {
    if (!isTauri() || eatEnabled === null || eatBusy) {
      return;
    }
    setEatBusy(true);
    try {
      const next = await invoke<boolean>("set_hid_tap_eat", {
        enabled: !eatEnabled,
      });
      setEatEnabled(next);
      setFeedback(
        next
          ? "已开启：系统不再响应遥控器按键，由本应用注入映射动作"
          : "已关闭：系统会同时响应遥控器按键",
      );
    } catch (err) {
      setFeedback(`切换失败：${err}`);
    } finally {
      setEatBusy(false);
    }
  }

  useEffect(() => {
    if (voiceTarget !== "windows_voice") {
      setDriverOpen(false);
      return;
    }
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
  }, [voiceTarget]);

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (targetRef.current && !targetRef.current.contains(e.target as Node)) {
        setTargetOpen(false);
      }
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

  async function connect() {
    if (!isTauri()) {
      setFeedback("请在桌面应用内连接");
      return;
    }
    setConnecting(true);
    setFeedback("正在连接并枚举 GATT 特征…");
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      const endpointsReady = Boolean(result.endpoints.audio && result.endpoints.control);
      markConnected(endpointsReady);
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
        setFeedback(bridgeRes);
      } catch (bridgeErr) {
        setFeedback(`连接成功，但语音桥启动失败：${bridgeErr}`);
      }
    } catch (err) {
      setFeedback(`连接失败：${err}`);
    } finally {
      setConnecting(false);
    }
  }

  async function runVoiceSimulation() {
    if (voiceTarget !== "windows_voice") {
      setSimResult("第三方输入法模拟尚未接入");
      return;
    }
    if (!isTauri()) {
      setSimResult("浏览器预览：请在桌面应用内模拟");
      return;
    }
    simInputRef.current?.focus();
    setSimResult("正在模拟：Win+H → 合成语音 → CABLE…");
    try {
      const ret = await invoke<{
        frames: number;
        pcm_samples: number;
        output_samples: number;
        win_h_toast: boolean;
        test_audio: string;
        test_audio_ms: number;
      }>("simulate_voice_chain", { outputDevice: "CABLE Input" });
      setSimResult(
        `模拟完成：${ret.frames} 帧，PCM ${ret.pcm_samples}，输出 ${ret.output_samples} 样本，测试音频 ${ret.test_audio}（${ret.test_audio_ms}ms），Win+H=${ret.win_h_toast}`
      );
      invoke("log_message", {
        message: `模拟结束，输入框内容=${JSON.stringify(simInputRef.current?.value ?? "")}`,
      }).catch(() => {});
    } catch (err) {
      setSimResult(`模拟失败：${err}`);
    }
  }

  async function triggerVoiceTyping() {
    if (!isTauri()) {
      setSimResult("浏览器预览：请在桌面应用内操作");
      return;
    }
    try {
      const res = await invoke<string>("trigger_voice_typing");
      setSimResult(res);
    } catch (err) {
      setSimResult(`唤出失败：${err}`);
    }
  }

  const imeName =
    voiceTarget === "ime_wechat"
      ? "微信输入法"
      : voiceTarget === "ime_doubao"
        ? "豆包输入法"
        : voiceTarget === "ime_sogou"
          ? "搜狗输入法"
          : "第三方输入法";

  const briefs: { key: string; label: string; tone: string; title?: string }[] = [
    {
      key: "bridge",
      label: "ATVV 语音桥",
      tone: bridgeStatus === "running" ? "ok" : "warn",
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
    <div className="page">
      <section className="card device-card">
        <div className="device-top">
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
        </div>
        {feedback && <p className="hint device-feedback">{feedback}</p>}
        {tapMessage && <p className="hint device-feedback">{tapMessage}</p>}
        <div className="device-eat-row">
          <div className="device-eat-info">
            <div className="device-eat-title">拦截 HID 按键信号</div>
            <p className="hint">
              {eatEnabled === null
                ? "读取中…"
                : eatEnabled
                  ? "已开启：系统不响应遥控器按键，只由本应用注入映射动作"
                  : "已关闭：系统会同时响应遥控器按键"}
            </p>
          </div>
          <button
            className={`btn${eatEnabled ? "" : " primary"}`}
            onClick={toggleEat}
            disabled={eatEnabled === null || eatBusy || !isTauri()}
          >
            {eatBusy ? "切换中…" : eatEnabled ? "关闭" : "开启"}
          </button>
        </div>
        <div className="device-status">
          {briefs.map((b) => (
            <span
              key={b.key}
              className={`device-status-pill ${b.tone}`}
              title={b.title}
            >
              {b.label}
            </span>
          ))}
        </div>
      </section>

      <section className="card">
        <div className="card-title">识别方案</div>
        <div className="wizard-group">
          <div className="wizard-label">语音识别目标</div>
          <div className="status-select" ref={targetRef}>
            <button
              type="button"
              className={`status-select-trigger${targetOpen ? " open" : ""}`}
              onClick={() => setTargetOpen((open) => !open)}
            >
              <span
                className={`status-dot ${
                  TARGET_OPTIONS.find((target) => target.value === voiceTarget)?.status ?? "preview"
                }`}
              />
              <span>
                {TARGET_OPTIONS.find((target) => target.value === voiceTarget)?.label}
              </span>
              <span className="status-select-caret">▾</span>
            </button>
            {targetOpen && (
              <div className="status-select-menu">
                {TARGET_OPTIONS.map((target) => (
                  <button
                    type="button"
                    key={target.value}
                    className="status-option"
                    onClick={() => {
                      setVoiceTarget(target.value);
                      setTargetOpen(false);
                    }}
                  >
                    <span className={`status-dot ${target.status}`} />
                    <span>{target.label}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
        {voiceTarget === "windows_voice" && (
          <>
            <div className="wizard-group">
              <div className="wizard-label">虚拟声卡</div>
              <div className="status-select" ref={driverRef}>
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
            <p className="hint">当前音频出口：{selected}。</p>
            <p className="hint">首次使用：按 Win+H 唤出语音条，在 ⚙️ 设置中把麦克风选为 CABLE Output（Windows 会记住，无需改系统默认麦克风）。</p>
          </>
        )}
      </section>

      {voiceTarget === "windows_voice" && (
        <section className="card sim-card">
          <textarea
            ref={simInputRef}
            className="sim-input"
            rows={3}
            placeholder="点击「模拟完整语音链」后，Windows 语音键入会以此处为输入目标"
          />
          <div className="sim-actions">
            <button className="btn primary" onClick={runVoiceSimulation} disabled={!isTauri()}>
              {voiceTarget === "windows_voice"
                ? "模拟完整语音链（无遥控器）"
                : `模拟 ${imeName}（未接入）`}
            </button>
            <button className="btn" onClick={triggerVoiceTyping} disabled={!isTauri()}>
              🎙️ 唤出语音输入条（Win + H）
            </button>
          </div>
        </section>
      )}

      {simResult && (
        <section className="card">
          <div className="card-title">模拟语音链结果</div>
          <p>{simResult}</p>
        </section>
      )}
    </div>
  );
}
