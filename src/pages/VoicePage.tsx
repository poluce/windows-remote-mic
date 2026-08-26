import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

export function VoicePage() {
  const [voiceTarget, setVoiceTarget] = useState("windows_voice");
  const [virtualDriver, setVirtualDriver] = useState("vb_cable");
  const [selected] = useState("CABLE 输入（VB-CABLE）");
  const [simResult, setSimResult] = useState("");

  async function runVoiceSimulation() {
    if (voiceTarget !== "windows_voice") {
      setSimResult("第三方输入法模拟尚未接入");
      return;
    }
    if (!isTauri()) {
      setSimResult("浏览器预览：请在桌面应用内模拟");
      return;
    }
    setSimResult("正在模拟：Win+H → 合成语音 → CABLE…");
    try {
      const ret = await invoke<{
        frames: number;
        pcm_samples: number;
        output_samples: number;
        win_h_toast: boolean;
      }>("simulate_voice_chain", { outputDevice: "CABLE Input" });
      setSimResult(
        `模拟完成：${ret.frames} 帧，PCM ${ret.pcm_samples}，输出 ${ret.output_samples} 样本，Win+H=${ret.win_h_toast}`
      );
    } catch (err) {
      setSimResult(`模拟失败：${err}`);
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
          <select
            className="select"
            value={voiceTarget}
            onChange={(e) => setVoiceTarget(e.currentTarget.value)}
          >
            <option value="windows_voice">Windows 语音键入（Win + H）</option>
            <option value="ime_wechat">微信输入法（预留）</option>
            <option value="ime_doubao">豆包输入法（预留）</option>
            <option value="ime_sogou">搜狗输入法（预留）</option>
          </select>
        </div>
        {voiceTarget === "windows_voice" && (
          <>
            <div className="wizard-group">
              <div className="wizard-label">虚拟声卡</div>
              <select
                className="select"
                value={virtualDriver}
                onChange={(e) => setVirtualDriver(e.currentTarget.value)}
              >
                <option value="vb_cable">VB-CABLE（当前）</option>
                <option value="voicemeeter" disabled>
                  Voicemeeter（预留）
                </option>
                <option value="rearoute" disabled>
                  ReaRoute（预留）
                </option>
              </select>
            </div>
            <p className="hint">当前音频出口：{selected}</p>
          </>
        )}
      </section>

      <section className="card actions">
        <button className="btn primary" onClick={runVoiceSimulation} disabled={!isTauri()}>
          {voiceTarget === "windows_voice"
            ? "模拟完整语音链（无遥控器）"
            : `模拟 ${imeName}（未接入）`}
        </button>
        <button className="btn" disabled>
          打开系统语音设置
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
