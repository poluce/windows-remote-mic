import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

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

export function VoicePage() {
  const [voiceTarget, setVoiceTarget] = useState("windows_voice");
  const [virtualDriver, setVirtualDriver] = useState("vb_cable");
  const [selected] = useState("CABLE 输入（VB-CABLE）");
  const [simResult, setSimResult] = useState("");
  const [driverStatus, setDriverStatus] = useState<DriverStatus>("unknown");
  const [driverOpen, setDriverOpen] = useState(false);
  const [targetOpen, setTargetOpen] = useState(false);
  const driverRef = useRef<HTMLDivElement>(null);
  const targetRef = useRef<HTMLDivElement>(null);
  const simInputRef = useRef<HTMLTextAreaElement>(null);

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

  return (
    <div className="page">

      <section className="card">
        <div className="card-title">语音识别目标</div>
        <div className="voice-route">
          <div className="route-node">
            <span>🎛️</span>
            <div>遥控器</div>
            <small>RC003</small>
          </div>
          {voiceTarget === "windows_voice" ? (
            <>
              <span className="route-arrow flow">→</span>
              <div className="route-node">
                <span>🎧</span>
                <div>
                  {virtualDriver === "vb_cable"
                    ? "VB-CABLE"
                    : virtualDriver === "voicemeeter"
                      ? "Voicemeeter"
                      : virtualDriver === "rearoute"
                        ? "ReaRoute"
                        : "虚拟声卡"}
                </div>
                <small>{virtualDriver === "vb_cable" ? selected : "预留"}</small>
              </div>
              <span className="route-arrow flow">→</span>
            </>
          ) : (
            <span className="route-arrow flow">→</span>
          )}
          <div className="route-node active">
            <span>🎙️</span>
            <div>
              {voiceTarget === "windows_voice"
                ? "Windows 语音键入"
                : voiceTarget === "ime_wechat"
                  ? "微信输入法"
                  : voiceTarget === "ime_doubao"
                    ? "豆包输入法"
                    : voiceTarget === "ime_sogou"
                      ? "搜狗输入法"
                      : "第三方输入法"}
            </div>
            <small>{voiceTarget === "windows_voice" ? "Win + H" : "预留"}</small>
          </div>
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
        <section className="card">
          <div className="card-title">模拟输入框</div>
          <textarea
            ref={simInputRef}
            className="sim-input"
            rows={3}
            placeholder="点击「模拟完整语音链」后，Windows 语音键入会以此处为输入目标"
          />
          <p className="hint">模拟会自动聚焦此输入框；使用真实语音样本测试，识别文字会显示在这里。</p>
        </section>
      )}

      <section className="card actions">
        <button className="btn primary" onClick={runVoiceSimulation} disabled={!isTauri()}>
          {voiceTarget === "windows_voice"
            ? "模拟完整语音链（无遥控器）"
            : `模拟 ${imeName}（未接入）`}
        </button>
        <button className="btn" onClick={triggerVoiceTyping} disabled={!isTauri()}>
          🎙️ 唤出语音输入条（Win + H）
        </button>
      </section>

      {simResult && (
        <section className="card">
          <div className="card-title">模拟语音链结果</div>
          <p>{simResult}</p>
        </section>
      )}
    </div>
  );
}
