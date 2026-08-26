import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "./Xiaomi2ProRemote";
import "./RemoteKeyTester.css";

export type KeyCalibration = {
  button: string;
  code: string;
  key: string;
  vkey?: number;
};

export const REMOTE_KEYS: Array<{ id: string; name: string; defaultCode: string }> = [
  { id: "power", name: "电源", defaultCode: "Escape" },
  { id: "mic", name: "麦克风", defaultCode: "F5" },
  { id: "up", name: "上", defaultCode: "ArrowUp" },
  { id: "down", name: "下", defaultCode: "ArrowDown" },
  { id: "left", name: "左", defaultCode: "ArrowLeft" },
  { id: "right", name: "右", defaultCode: "ArrowRight" },
  { id: "ok", name: "确定", defaultCode: "Enter" },
  { id: "back", name: "返回", defaultCode: "BrowserBack" },
  { id: "home", name: "主页", defaultCode: "BrowserHome" },
  { id: "menu", name: "菜单", defaultCode: "ContextMenu" },
  { id: "tv", name: "TV", defaultCode: "LaunchApp1" },
  { id: "volume_up", name: "音量 +", defaultCode: "AudioVolumeUp" },
  { id: "volume_down", name: "音量 −", defaultCode: "AudioVolumeDown" },
];

type TriggerRecord = {
  single: boolean;
  double: boolean;
  long: boolean;
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

export function RemoteKeyTester() {
  const [mode, setMode] = useState<"live" | "guided">("live");
  const [active, setActive] = useState(false);
  const [calibrations, setCalibrations] = useState<Record<string, KeyCalibration>>({});

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

  // 映射 Code -> ButtonId
  function resolveButton(code: string): string | null {
    // 1. 优先查用户校准表
    for (const [btnId, cal] of Object.entries(calibrations)) {
      if (cal.code === code) return btnId;
    }
    // 2. 回退查默认表
    const def = REMOTE_KEYS.find((k) => k.defaultCode === code);
    return def ? def.id : null;
  }

  // 监听键盘事件
  useEffect(() => {
    if (!active) return;

    function handleKeyDown(e: KeyboardEvent) {
      // 彻底拦截遥控器/多媒体按键的浏览器默认行为（防止网页后退、滚动、刷新等）
      if (INTERCEPT_CODES.has(e.code) || INTERCEPT_CODES.has(e.key)) {
        e.preventDefault();
        e.stopPropagation();
      }

      const now = performance.now();
      const code = e.code || e.key;

      if (mode === "guided") {
        // 向导模式：采集当前步骤按键
        if (guidedIndex < REMOTE_KEYS.length) {
          const target = REMOTE_KEYS[guidedIndex];
          const newCal: KeyCalibration = {
            button: target.id,
            code: e.code,
            key: e.key,
            vkey: e.keyCode,
          };
          const nextCalibs = { ...calibrations, [target.id]: newCal };
          setCalibrations(nextCalibs);

          const nextDone = Array.from(new Set([...guidedDoneKeys, target.id]));
          setGuidedDoneKeys(nextDone);

          const nextIdx = guidedIndex + 1;
          if (nextIdx < REMOTE_KEYS.length) {
            setGuidedIndex(nextIdx);
          } else {
            // 全部 13 键采集完毕 ➔ 自动保存并切回快速测试（测试按钮回到未开始状态）
            setGuidedIndex(nextIdx);
            if (isTauri()) {
              invoke("save_key_calibrations", { calibrations: nextCalibs })
                .then(() => {
                  setTimeout(() => {
                    setMode("live");
                    setActive(false);
                    setCurrentKey(null);
                  }, 800);
                })
                .catch(() => {});
            }
          }
        }
        return;
      }

      // 自由识别模式
      const btnId = resolveButton(code);
      if (btnId) {
        setCurrentKey(btnId);
        if (!pressTimes.current[btnId]) {
          pressTimes.current[btnId] = now;
        }
      }
    }

    function handleKeyUp(e: KeyboardEvent) {
      if (mode === "guided") return;

      const now = performance.now();
      const code = e.code || e.key;
      const btnId = resolveButton(code);
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

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  }, [active, mode, guidedIndex, calibrations, guidedDoneKeys]);

  function switchToLive() {
    setMode("live");
    setActive(false);
    setCurrentKey(null);
  }

  function startGuided() {
    setMode("guided");
    setActive(true);
    setGuidedIndex(0);
    setGuidedDoneKeys([]);
  }

  function resetToDefaults() {
    setCalibrations({});
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
            onClick={startGuided}
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
              <div className="live-actions-bar">
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
              </div>

              {/* 13 键触发矩阵 */}
              <div>
                <div className="key-matrix-grid">
                  {REMOTE_KEYS.map((k) => {
                    const rec = matrix[k.id] || { single: false, double: false, long: false };
                    const allPass = rec.single && rec.double && rec.long;
                    const isCalibrated = !!calibrations[k.id];
                    return (
                      <div
                        key={k.id}
                        className={`matrix-card ${allPass ? "all-passed" : ""}`}
                      >
                        <div className="matrix-row-left">
                          <span className="matrix-name">{k.name}</span>
                          {isCalibrated && (
                            <span className="hint" style={{ fontSize: 10, color: "#16a34a" }}>
                              ✓
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
                      : "全部校准完成"}
                  </span>
                  <span className="guided-count">
                    {Math.min(guidedIndex + 1, REMOTE_KEYS.length)} / {REMOTE_KEYS.length}
                  </span>
                </div>
                <div className="guided-actions">
                  <button
                    className="btn"
                    onClick={() =>
                      setGuidedIndex((prev) =>
                        Math.min(prev + 1, REMOTE_KEYS.length - 1)
                      )
                    }
                  >
                    跳过此键
                  </button>
                  <button className="btn" onClick={resetToDefaults}>
                    重置默认
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
            </>
          )}
        </div>
      </div>
    </section>
  );
}