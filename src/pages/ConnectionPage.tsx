import { invoke, isTauri } from "@tauri-apps/api/core";
import { useState } from "react";

import {
  bridgeStateLabel,
  tapStatusLabel,
  useEffectiveConnected,
  useRuntimeStatus,
} from "../store/runtimeStatus";

import { IconBluetooth, IconMic } from "../components/icons";
import { DRIVER_OPTIONS, useVbCableDriver } from "./connection/useVbCableDriver";
import { useConnectionActions } from "./connection/useConnectionActions";

import "./ConnectionPage.css";

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
  // 设备动作（扫描/连接/断开/重试 + 自动连接）：副作用集中在 hook 里。
  const {
    scanning,
    connecting,
    disconnecting,
    feedback,
    scan,
    connect,
    retryBridge,
    disconnect,
  } = useConnectionActions();
  // 虚拟声卡下拉框的本地交互状态。
  const {
    virtualDriver,
    setVirtualDriver,
    driverStatus,
    driverOpen,
    setDriverOpen,
    driverRef,
  } = useVbCableDriver();

  // 「下一步」卡里的语音输入按钮（引导首次使用的人把麦克风绑定到 CABLE Output）。
  const [voiceMsg, setVoiceMsg] = useState("");

  async function triggerVoiceTyping() {
    if (!isTauri()) {
      setVoiceMsg("浏览器预览：请在桌面应用内操作");
      return;
    }
    try {
      setVoiceMsg(await invoke<string>("trigger_voice_typing"));
    } catch (err) {
      setVoiceMsg(`唤出失败：${err}`);
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
    <div className="page">
      <div className="page-head">
        <div>
          <h1>连接</h1>
          <p>扫描并连接遥控器，查看语音链路与按键接管状态。</p>
        </div>
      </div>

      <div className="conn-grid">
      <div className="conn-col">
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
              className={`pill ${effectiveConnected ? "ok" : "warn"}`}
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
              className={`pill ${b.tone}`}
              title={b.title}
            >
              <span className="pill-dot" />
              {b.label}
            </span>
          ))}
        </div>
        {silentSessionHint && (
          <div className="callout err">
            <span className="callout-title">遥控器语音服务可能已挂起</span>
            <span className="callout-body">
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
          <div className={`callout ${bridgeState === "failed" ? "err" : ""}`}>
            <span className="callout-title">
              {bridgeState === "failed"
                ? "语音桥连接失败"
                : bridgeState === "waiting"
                  ? "等待遥控器上线"
                  : bridgeState === "starting"
                    ? "正在建立语音链路…"
                    : "语音桥正在重试"}
            </span>
            <span className="callout-body">
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
              <div className="actions" style={{ marginTop: 8 }}>
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
          <div className="callout">
            <span className="callout-title">语音桥已停止</span>
            <span className="callout-body">
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

      {/* 下一步：未连接时给上手三步，已就绪时给使用提示。
          语音桥在等待 / 重试 / 失败时，上面的卡片已经有对应说明，这里不重复。 */}
      {!effectiveConnected && (
        <section className="card next-card">
          <div className="next-card-head">
            <div>
              <div className="next-card-title">还没连上遥控器</div>
              <p className="next-card-sub">
                按下面三步走，一般一分钟内就能开始说话。
              </p>
            </div>
            <button
              className="btn primary"
              onClick={scan}
              disabled={!isTauri() || scanning}
            >
              {scanning ? "扫描中…" : "扫描遥控器"}
            </button>
          </div>
          <ol className="next-steps">
            <li className="next-step">
              <span className="next-no">1</span>
              <div>
                <div className="next-title">唤醒遥控器</div>
                <div className="next-detail">
                  按一下遥控器任意键，指示灯亮起即已唤醒（长时间不用会休眠）。
                </div>
              </div>
            </li>
            <li className="next-step">
              <span className="next-no">2</span>
              <div>
                <div className="next-title">在 Windows 蓝牙里配对</div>
                <div className="next-detail">
                  长按「主页 + 菜单」直到指示灯快闪，再到系统蓝牙设置里添加并连接 RC003。
                </div>
              </div>
            </li>
            <li className="next-step">
              <span className="next-no">3</span>
              <div>
                <div className="next-title">回到本页连接</div>
                <div className="next-detail">
                  点「扫描」找到 RC003 后点「连接」；连上后长按麦克风键即可说话。
                </div>
              </div>
            </li>
          </ol>
        </section>
      )}

      {effectiveConnected && bridgeState === "running" && (
        <section className="card next-card">
          <div className="next-card-head">
            <div>
              <div className="next-card-title">链路已就绪，可以开始用了</div>
              <p className="next-card-sub">
                长按遥控器麦克风键说话，松开结束；识别出的文字会输入到当前光标处。
              </p>
            </div>
            <button
              className="btn"
              onClick={triggerVoiceTyping}
              disabled={!isTauri()}
            >
              <IconMic size={14} />
              唤出语音输入条
            </button>
          </div>
          <ol className="next-steps">
            <li className="next-step">
              <span className="next-no">1</span>
              <div>
                <div className="next-title">首次使用需要绑一次麦克风</div>
                <div className="next-detail">
                  按 Win+H 打开语音条 →「设置」→ 麦克风选「CABLE Output」。
                  <span className="next-note">一次即可，永久记忆；电脑自带麦克风不受影响。</span>
                </div>
              </div>
            </li>
            <li className="next-step">
              <span className="next-no">2</span>
              <div>
                <div className="next-title">说话没反应？</div>
                <div className="next-detail">
                  到「诊断 → 按键测试」确认 13 个键都能收到信号，再检查虚拟声卡是否为「CABLE Input」。
                </div>
              </div>
            </li>
            <li className="next-step">
              <span className="next-no">3</span>
              <div>
                <div className="next-title">想改按键动作</div>
                <div className="next-detail">
                  在「按键映射」页可改 13 个键的动作，也能按前台应用分别设置。
                </div>
              </div>
            </li>
          </ol>
          {voiceMsg && <p className="hint">{voiceMsg}</p>}
        </section>
      )}
    </div>
  );
}
