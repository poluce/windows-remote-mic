import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Xiaomi2ProRemote } from "./Xiaomi2ProRemote";
import "./RemoteKeyTester.css";

export type KeyCalibration = {
  button: string;
  code: string;
  key: string;
  vkey?: number;
};

export const REMOTE_KEYS: Array<{
  id: string;
  name: string;
  defaultCode: string;
  aliases?: string[];
  defaultVKey?: number;
}> = [
  { id: "power", name: "电源", defaultCode: "Escape", aliases: ["Power", "Sleep", "SystemSleep", "Escape", "BrowserStop"], defaultVKey: 27 },
  { id: "mic", name: "麦克风", defaultCode: "F5", aliases: ["F5", "VoiceCommand", "Microphone"], defaultVKey: 116 },
  { id: "up", name: "上", defaultCode: "ArrowUp", aliases: ["ArrowUp", "Up"], defaultVKey: 38 },
  { id: "down", name: "下", defaultCode: "ArrowDown", aliases: ["ArrowDown", "Down"], defaultVKey: 40 },
  { id: "left", name: "左", defaultCode: "ArrowLeft", aliases: ["ArrowLeft", "Left"], defaultVKey: 37 },
  { id: "right", name: "右", defaultCode: "ArrowRight", aliases: ["ArrowRight", "Right"], defaultVKey: 39 },
  { id: "ok", name: "确定", defaultCode: "Enter", aliases: ["Enter", "NumpadEnter", "Select"], defaultVKey: 13 },
  { id: "back", name: "返回", defaultCode: "BrowserBack", aliases: ["BrowserBack", "Back", "Escape", "Backspace", "GoBack"], defaultVKey: 166 },
  { id: "home", name: "主页", defaultCode: "BrowserHome", aliases: ["BrowserHome", "Home", "LaunchApplication2", "LaunchApp2"], defaultVKey: 172 },
  { id: "menu", name: "菜单", defaultCode: "ContextMenu", aliases: ["ContextMenu", "Apps", "Menu", "F10"], defaultVKey: 93 },
  { id: "tv", name: "TV", defaultCode: "LaunchApp1", aliases: ["LaunchApp1", "LaunchMail", "LaunchApplication1", "TV", "Guide"], defaultVKey: 180 },
  { id: "volume_up", name: "音量 +", defaultCode: "AudioVolumeUp", aliases: ["AudioVolumeUp", "VolumeUp", "VK_175", "Volume_Up", "AudioVolumeIncrement", "VolumeIncrement"], defaultVKey: 175 },
  { id: "volume_down", name: "音量 −", defaultCode: "AudioVolumeDown", aliases: ["AudioVolumeDown", "VolumeDown", "VK_174", "Volume_Down", "AudioVolumeDecrement", "VolumeDecrement"], defaultVKey: 174 },
];

type TriggerRecord = {
  single: boolean;
  double: boolean;
  long: boolean;
};

const VK_DOM: Record<number, { code: string; key: string }> = {
  13: { code: "Enter", key: "Enter" },
  27: { code: "Escape", key: "Escape" },
  37: { code: "ArrowLeft", key: "ArrowLeft" },
  38: { code: "ArrowUp", key: "ArrowUp" },
  39: { code: "ArrowRight", key: "ArrowRight" },
  40: { code: "ArrowDown", key: "ArrowDown" },
  93: { code: "ContextMenu", key: "ContextMenu" },
  116: { code: "F5", key: "F5" },
  166: { code: "BrowserBack", key: "BrowserBack" },
  172: { code: "BrowserHome", key: "BrowserHome" },
  173: { code: "AudioVolumeMute", key: "AudioVolumeMute" },
  174: { code: "AudioVolumeDown", key: "AudioVolumeDown" },
  175: { code: "AudioVolumeUp", key: "AudioVolumeUp" },
  180: { code: "LaunchApp1", key: "LaunchApp1" },
  182: { code: "LaunchApp1", key: "LaunchApp1" },
  255: { code: "Power", key: "Unidentified" },
};

const INTERCEPT_CODES = new Set([
  // 基础控制
  "Escape",
  "Enter",
  "F5",
  "Space",
  "Tab",
  "Backspace",
  "ContextMenu",
  // 方向导航
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  // 浏览器控制（重点防后退）
  "BrowserBack",
  "BrowserForward",
  "BrowserHome",
  "BrowserRefresh",
  "BrowserSearch",
  "BrowserFavorites",
  "BrowserStop",
  // 多媒体与音量
  "AudioVolumeUp",
  "AudioVolumeDown",
  "AudioVolumeMute",
  "MediaTrackNext",
  "MediaTrackPrevious",
  "MediaStop",
  "MediaPlayPause",
  // 应用快捷键
  "LaunchApp1",
  "LaunchApp2",
  "LaunchMail",
  "LaunchMediaPlayer",
]);

const TYPING_CODES = new Set([
  "Space",
  "Tab",
  "Backspace",
  "Delete",
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "CapsLock",
  "NumLock",
  "ScrollLock",
  "Process",
]);

const COLLECTABLE_CODES = new Set<string>([
  "Power",
  "Sleep",
]);
const COLLECTABLE_VKEYS = new Set<number>([255]);
for (const k of REMOTE_KEYS) {
  COLLECTABLE_CODES.add(k.defaultCode);
  if (k.defaultVKey) COLLECTABLE_VKEYS.add(k.defaultVKey);
  for (const a of k.aliases || []) {
    if (a !== "Backspace") COLLECTABLE_CODES.add(a);
  }
}
for (const [vk, mapped] of Object.entries(VK_DOM)) {
  COLLECTABLE_VKEYS.add(Number(vk));
  COLLECTABLE_CODES.add(mapped.code);
  COLLECTABLE_CODES.add(mapped.key);
}
COLLECTABLE_CODES.delete("Backspace");

function isPcTypingKey(e: KeyboardEvent): boolean {
  if (e.isComposing || e.keyCode === 229) return true;
  const code = e.code || "";
  const key = e.key || "";
  const vk = e.keyCode || 0;
  if (TYPING_CODES.has(code) || TYPING_CODES.has(key)) return true;
  if (code.startsWith("Key") || code.startsWith("Digit")) return true;
  if (code.startsWith("Numpad") && code !== "NumpadEnter") return true;
  if (key.length === 1) return true;
  if ((vk >= 65 && vk <= 90) || (vk >= 48 && vk <= 57) || vk === 8 || vk === 9 || vk === 32) return true;
  if (code.startsWith("VK_") && ((vk >= 65 && vk <= 90) || (vk >= 48 && vk <= 57) || vk === 8 || vk === 32)) {
    return true;
  }
  return false;
}

function isCollectableRemoteKey(e: KeyboardEvent): boolean {
  if (isPcTypingKey(e)) return false;
  const code = e.code || "";
  const key = e.key || "";
  const vk = e.keyCode || 0;
  if (vk && COLLECTABLE_VKEYS.has(vk)) return true;
  if (code && COLLECTABLE_CODES.has(code)) return true;
  if (key && COLLECTABLE_CODES.has(key)) return true;
  return false;
}

export function RemoteKeyTester() {
  const [mode, setMode] = useState<"live" | "guided">("live");
  const [active, setActive] = useState(false);
  const [calibrations, setCalibrations] = useState<Record<string, KeyCalibration>>({});
  const [lastRawKey, setLastRawKey] = useState<string>("");

  // 自由模式状态
  const [currentKey, setCurrentKey] = useState<string | null>(null);
  const [matrix, setMatrix] = useState<Record<string, TriggerRecord>>(() => {
    const init: Record<string, TriggerRecord> = {};
    for (const k of REMOTE_KEYS) {
      init[k.id] = { single: false, double: false, long: false };
    }
    return init;
  });

  // 向导模式状态
  const [guidedIndex, setGuidedIndex] = useState(0);
  const [guidedDoneKeys, setGuidedDoneKeys] = useState<string[]>([]);

  // 时序判定 Ref
  const pressTimes = useRef<Record<string, number>>({});
  const lastSingleRelease = useRef<Record<string, number>>({});
  const singleTimers = useRef<Record<string, number>>({});
  const modeRef = useRef(mode);
  const guidedIndexRef = useRef(guidedIndex);
  const calibrationsRef = useRef(calibrations);
  const guidedDoneKeysRef = useRef(guidedDoneKeys);
  const lastIngestRef = useRef({ vkey: -1, pressed: false, at: 0 });
  modeRef.current = mode;
  guidedIndexRef.current = guidedIndex;
  calibrationsRef.current = calibrations;
  guidedDoneKeysRef.current = guidedDoneKeys;

  // 加载已保存的校准表
  useEffect(() => {
    if (!isTauri()) return;
    invoke<Record<string, KeyCalibration>>("get_key_calibrations")
      .then((c) => {
        if (c && Object.keys(c).length > 0) {
          setCalibrations(c);
        }
      })
      .catch(() => {});
  }, []);

  // 映射按键事件 -> ButtonId
  function resolveButton(e: { code?: string; key?: string; keyCode?: number }): string | null {
    const code = e.code || "";
    const key = e.key || "";
    const vkey = e.keyCode || 0;

    // 1. 优先查用户校准表 (按 code > key > vkey 逐层匹配)
    for (const [btnId, cal] of Object.entries(calibrationsRef.current)) {
      if (code && cal.code && cal.code === code && cal.code !== "Unidentified") return btnId;
      if (key && cal.key && cal.key === key && cal.key !== "Unidentified") return btnId;
      if (vkey && cal.vkey && cal.vkey === vkey && cal.vkey !== 0) return btnId;
    }

    // 2. 回退查默认表和别名表
    for (const k of REMOTE_KEYS) {
      if (code && k.defaultCode === code) return k.id;
      if (key && k.defaultCode === key) return k.id;
      if (vkey && k.defaultVKey === vkey) return k.id;
      if (k.aliases) {
        if (code && k.aliases.includes(code)) return k.id;
        if (key && k.aliases.includes(key)) return k.id;
      }
    }
    return null;
  }

  // 监听键盘事件
  useEffect(() => {
    if (!active) return;

    function shouldIntercept(e: KeyboardEvent) {
      return (
        INTERCEPT_CODES.has(e.code) ||
        INTERCEPT_CODES.has(e.key) ||
        e.keyCode === 166 ||
        e.keyCode === 175 ||
        e.keyCode === 174 ||
        e.keyCode === 173 ||
        e.keyCode === 172 ||
        e.keyCode === 27 ||
        e.keyCode === 8 ||
        e.keyCode === 255
      );
    }

    function alreadyIngested(vkey: number, pressed: boolean) {
      const now = performance.now();
      const last = lastIngestRef.current;
      if (last.vkey === vkey && last.pressed === pressed && now - last.at < 80) {
        return true;
      }
      lastIngestRef.current = { vkey, pressed, at: now };
      return false;
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (shouldIntercept(e)) {
        e.preventDefault();
        e.stopPropagation();
      }
      if (e.repeat) return;
      if (!isCollectableRemoteKey(e)) return;
      if (alreadyIngested(e.keyCode || 0, true)) return;

      const now = performance.now();
      const codeVal = e.code && e.code !== "Unidentified" ? e.code : (e.key && e.key !== "Unidentified" ? e.key : `VK_${e.keyCode}`);
      const keyVal = e.key && e.key !== "Unidentified" ? e.key : codeVal;

      const logStr = `[tester] keydown received: code='${e.code || ""}', key='${e.key || ""}', keyCode=${e.keyCode}`;
      setLastRawKey(`code: ${e.code || "空"}, key: ${e.key || "空"}, vkey: ${e.keyCode}`);
      if (isTauri()) {
        invoke("log_message", { message: logStr }).catch(() => {});
      }

      if (modeRef.current === "guided") {
        const guidedIndex = guidedIndexRef.current;
        if (guidedIndex < REMOTE_KEYS.length) {
          const target = REMOTE_KEYS[guidedIndex];
          const newCal: KeyCalibration = {
            button: target.id,
            code: codeVal,
            key: keyVal,
            vkey: e.keyCode || 0,
          };
          const nextCalibs = { ...calibrationsRef.current, [target.id]: newCal };
          setCalibrations(nextCalibs);

          const nextDone = Array.from(new Set([...guidedDoneKeysRef.current, target.id]));
          setGuidedDoneKeys(nextDone);

          const nextIdx = guidedIndex + 1;
          if (nextIdx < REMOTE_KEYS.length) {
            setGuidedIndex(nextIdx);
          } else {
            finishGuided(nextCalibs);
          }
        }
        return;
      }

      const btnId = resolveButton(e);
      if (btnId) {
        setCurrentKey(btnId);
        if (!pressTimes.current[btnId]) {
          pressTimes.current[btnId] = now;
        }
      }
    }

    function handleKeyUp(e: KeyboardEvent) {
      if (shouldIntercept(e)) {
        e.preventDefault();
        e.stopPropagation();
      }

      if (modeRef.current === "guided") return;
      if (!isCollectableRemoteKey(e)) return;
      if (alreadyIngested(e.keyCode || 0, false)) return;

      const now = performance.now();
      const btnId = resolveButton(e);
      if (!btnId) return;

      const pressedAt = pressTimes.current[btnId] || now;
      delete pressTimes.current[btnId];
      const heldMs = now - pressedAt;

      // 判定 LongPress
      if (heldMs >= 550) {
        setMatrix((prev) => ({
          ...prev,
          [btnId]: { ...prev[btnId], long: true },
        }));
        return;
      }

      // 判定 DoubleClick
      const lastRelease = lastSingleRelease.current[btnId] || 0;
      if (now - lastRelease <= 300) {
        clearTimeout(singleTimers.current[btnId]);
        delete lastSingleRelease.current[btnId];
        setMatrix((prev) => ({
          ...prev,
          [btnId]: { ...prev[btnId], double: true },
        }));
        return;
      }

      // 判定 SingleClick (等待 300ms 窗口)
      lastSingleRelease.current[btnId] = now;
      singleTimers.current[btnId] = window.setTimeout(() => {
        delete lastSingleRelease.current[btnId];
        setMatrix((prev) => ({
          ...prev,
          [btnId]: { ...prev[btnId], single: true },
        }));
      }, 300);
    }

    function handleMouseBack(e: MouseEvent | PointerEvent) {
      // 拦截鼠标第 4 / 5 侧键（Windows Consumer Back / Forward 常用通道）
      if (e.button === 3 || e.button === 4 || e.button === 5 || e.which === 4 || e.which === 5) {
        e.preventDefault();
        e.stopPropagation();
        if (isTauri()) {
          invoke("log_message", { message: `[tester] mouse back button received: button=${e.button}, which=${e.which}` }).catch(() => {});
        }
        const fakeKey = {
          code: "BrowserBack",
          key: "BrowserBack",
          keyCode: 166,
          repeat: false,
          preventDefault: () => {},
          stopPropagation: () => {},
        } as unknown as KeyboardEvent;
        handleKeyDown(fakeKey);
        setTimeout(() => handleKeyUp(fakeKey), 100);
      }
    }

    let unlistenFn: (() => void) | null = null;
    let listenCancelled = false;
    if (isTauri()) {
      listen<{ vkey: number; pressed: boolean }>("raw-remote-key", (event) => {
        const { vkey, pressed } = event.payload;
        const mapped = VK_DOM[vkey];
        const fakeKey = {
          code: mapped?.code || `VK_${vkey}`,
          key: mapped?.key || `VK_${vkey}`,
          keyCode: vkey,
          repeat: false,
          preventDefault: () => {},
          stopPropagation: () => {},
        } as unknown as KeyboardEvent;
        if (pressed) {
          handleKeyDown(fakeKey);
        } else {
          handleKeyUp(fakeKey);
        }
      }).then((fn) => {
        if (listenCancelled) {
          fn();
          return;
        }
        unlistenFn = fn;
      });
    }

    function trapBack() {
      window.history.pushState({ remoteMic: true }, "", window.location.href);
    }
    trapBack();
    window.addEventListener("popstate", trapBack);

    window.addEventListener("keydown", handleKeyDown, { capture: true });
    window.addEventListener("keyup", handleKeyUp, { capture: true });
    window.addEventListener("auxclick", handleMouseBack, { capture: true });
    window.addEventListener("mouseup", handleMouseBack, { capture: true });
    window.addEventListener("pointerdown", handleMouseBack, { capture: true });
    return () => {
      listenCancelled = true;
      if (unlistenFn) unlistenFn();
      window.removeEventListener("popstate", trapBack);
      window.removeEventListener("keydown", handleKeyDown, { capture: true });
      window.removeEventListener("keyup", handleKeyUp, { capture: true });
      window.removeEventListener("auxclick", handleMouseBack, { capture: true });
      window.removeEventListener("mouseup", handleMouseBack, { capture: true });
      window.removeEventListener("pointerdown", handleMouseBack, { capture: true });
    };
  }, [active]);

  function switchToLive() {
    setMode("live");
    setActive(false);
    setCurrentKey(null);
  }

  function startGuided(initialIdx = 0) {
    setMode("guided");
    setActive(true);
    setGuidedIndex(initialIdx);
  }

  function finishGuided(calibsToSave = calibrations) {
    if (isTauri()) {
      invoke("save_key_calibrations", { calibrations: calibsToSave }).catch(() => {});
    }
    // 校准完成时保持在当前页面展示完成状态，并同步持久化
    setGuidedIndex(REMOTE_KEYS.length);
  }

  function skipCurrentKey() {
    const nextIdx = guidedIndex + 1;
    if (nextIdx < REMOTE_KEYS.length) {
      setGuidedIndex(nextIdx);
    } else {
      finishGuided(calibrations);
    }
  }

  function resetToDefaults() {
    setCalibrations({});
    setGuidedDoneKeys([]);
    if (isTauri()) {
      invoke("save_key_calibrations", { calibrations: {} });
    }
  }

  const currentGuidedTarget =
    mode === "guided" && guidedIndex < REMOTE_KEYS.length
      ? REMOTE_KEYS[guidedIndex]
      : null;

  return (
    <section className="card remote-tester-card">
      <div className="tester-header">
        <div className="tester-modes">
          <button
            className={`mode-tab ${mode === "live" ? "active" : ""}`}
            onClick={switchToLive}
          >
            快速测试
          </button>
          <button
            className={`mode-tab ${mode === "guided" ? "active" : ""}`}
            onClick={() => startGuided(0)}
          >
            逐键校准
          </button>
        </div>
      </div>

      <div className="tester-body">
        {/* 左侧仿真遥控器 */}
        <div className="tester-remote-pane">
          <Xiaomi2ProRemote
            selected={mode === "live" ? currentKey || undefined : undefined}
            targetKey={currentGuidedTarget ? currentGuidedTarget.id : undefined}
            doneKeys={mode === "guided" ? guidedDoneKeys : undefined}
            onSelect={(k) => {
              if (mode === "guided") {
                const idx = REMOTE_KEYS.findIndex((item) => item.id === k);
                if (idx !== -1) setGuidedIndex(idx);
              }
            }}
          />
        </div>

        {/* 右侧交互与看板区 */}
        <div className="tester-info-pane">
          {/* 模式 1：自由测试模式 */}
          {mode === "live" && (
            <>
              <div className="live-actions-bar" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
                <button
                  className={`btn ${active ? "active" : "primary"}`}
                  onClick={() => {
                    if (!active) {
                      // 开始测试时自动清空之前的测试矩阵
                      const resetMat: Record<string, TriggerRecord> = {};
                      for (const k of REMOTE_KEYS) {
                        resetMat[k.id] = { single: false, double: false, long: false };
                      }
                      setMatrix(resetMat);
                      setCurrentKey(null);
                      setActive(true);
                    } else {
                      setActive(false);
                    }
                  }}
                >
                  {active ? "⏹ 停止测试" : "▶ 开始测试"}
                </button>
                {active && lastRawKey && (
                  <span className="hint" style={{ fontFamily: "monospace", fontSize: 11, background: "rgba(255,255,255,0.06)", padding: "4px 8px", borderRadius: 4 }}>
                    实时接收信号: {lastRawKey}
                  </span>
                )}
              </div>

              {/* 13 键触发矩阵 */}
              <div>
                <div className="key-matrix-grid">
                  {REMOTE_KEYS.map((k, idx) => {
                    const rec = matrix[k.id] || { single: false, double: false, long: false };
                    const allPass = rec.single && rec.double && rec.long;
                    const isCalibrated = !!calibrations[k.id];
                    return (
                      <div
                        key={k.id}
                        className={`matrix-card ${allPass ? "all-passed" : ""}`}
                        style={{ cursor: "pointer" }}
                        title="点击单独重新校准该键"
                        onClick={() => startGuided(idx)}
                      >
                        <div className="matrix-row-left">
                          <span className="matrix-name">{k.name}</span>
                          {isCalibrated && (
                            <span className="hint" style={{ fontSize: 10, color: "#16a34a" }}>
                              ✓ 已校准
                            </span>
                          )}
                        </div>
                        <div className="matrix-triggers">
                          <span className={`trig-pill ${rec.single ? "passed" : ""}`}>
                            单击
                          </span>
                          <span className={`trig-pill ${rec.double ? "passed" : ""}`}>
                            双击
                          </span>
                          <span className={`trig-pill ${rec.long ? "passed" : ""}`}>
                            长按
                          </span>
                        </div>
                      </div>
                    );
                  })}
                </div>
                <p className="hint" style={{ marginTop: 12, fontSize: 12 }}>
                  ℹ️ 提示：只采集遥控器按键，电脑键盘打字会被忽略。点击任意按键卡片可随时进入**单键重新校准**；【麦克风】键为 ATVV 专属蓝牙语音控制通道（直通 Win+H），不走常规键盘分发。
                </p>
              </div>
            </>
          )}

          {/* 模式 2：逐键校准模式 */}
          {mode === "guided" && (
            <>
              <div className="guided-top-bar">
                <div className="guided-current-info">
                  <span className="guided-badge">
                    {currentGuidedTarget
                      ? `请按【${currentGuidedTarget.name}】`
                      : "✓ 全部校准已保存"}
                  </span>
                  <span className="guided-count">
                    {Math.min(guidedIndex + 1, REMOTE_KEYS.length)} / {REMOTE_KEYS.length}
                  </span>
                </div>
                <div className="guided-actions">
                  {currentGuidedTarget ? (
                    <>
                      <button className="btn" onClick={skipCurrentKey}>
                        {guidedIndex >= REMOTE_KEYS.length - 1 ? "跳过并完成" : "跳过此键"}
                      </button>
                      <button className="btn primary" onClick={() => finishGuided(calibrations)}>
                        保存并完成
                      </button>
                    </>
                  ) : (
                    <button className="btn primary" onClick={switchToLive}>
                      ▶ 进入快速测试
                    </button>
                  )}
                  <button className="btn" onClick={resetToDefaults}>
                    重置为默认
                  </button>
                </div>
              </div>

              {/* 13 键校准状态矩阵 */}
              <div className="key-matrix-grid">
                {REMOTE_KEYS.map((k, idx) => {
                  const isCurrent = currentGuidedTarget?.id === k.id;
                  const isDone = guidedDoneKeys.includes(k.id) || !!calibrations[k.id];
                  return (
                    <div
                      key={k.id}
                      className={`matrix-card ${
                        isCurrent ? "guided-active-card" : isDone ? "all-passed" : ""
                      }`}
                      style={{ cursor: "pointer" }}
                      title="点击选择该按键重新录入"
                      onClick={() => setGuidedIndex(idx)}
                    >
                      <div className="matrix-row-left">
                        <span className="matrix-name">{k.name}</span>
                      </div>
                      <div className="matrix-triggers">
                        <span className={`trig-pill ${isDone ? "passed" : ""}`}>
                          {isDone ? "已采集 ✓" : isCurrent ? "录入中…" : "待校准"}
                        </span>
                      </div>
                    </div>
                  );
                })}
              </div>
              <p className="hint" style={{ marginTop: 12, fontSize: 12 }}>
                💡 只采集遥控器按键，电脑打字/退格不会写入校准。随时可点击任意按键卡片进行**单独重录**。
              </p>
            </>
          )}
        </div>
      </div>
    </section>
  );
}