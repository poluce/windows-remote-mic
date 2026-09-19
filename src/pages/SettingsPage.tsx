import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useRuntimeStatus } from "../store/runtimeStatus";

/**
 * 通用设置：按键相关的全局参数。
 *
 * 这些设置和「哪个应用在前台」无关，所以从按键映射页搬了出来——它们原先挤在
 * 映射向导的「触发方式」下面，看起来像是逐键设置，其实改一次对所有键生效。
 */
export function SettingsPage() {
  const [eatEnabled, setEatEnabled] = useState<boolean | null>(null);
  const [eatBusy, setEatBusy] = useState(false);
  const [longPressMs, setLongPressMs] = useState(550);
  const [doubleClickMs, setDoubleClickMs] = useState(300);
  const [msg, setMsg] = useState("");
  const { tapStatus } = useRuntimeStatus();

  useEffect(() => {
    if (!isTauri()) {
      setEatEnabled(true);
      return;
    }
    invoke<boolean>("get_hid_tap_eat")
      .then(setEatEnabled)
      .catch(() => setEatEnabled(null));
    invoke<{ long_press_ms: number; double_click_ms: number }>("get_trigger_timing")
      .then((t) => {
        setLongPressMs(t.long_press_ms);
        setDoubleClickMs(t.double_click_ms);
      })
      .catch(() => {});
  }, []);

  async function toggleEat() {
    if (!isTauri() || eatEnabled === null || eatBusy) return;
    setEatBusy(true);
    try {
      const next = await invoke<boolean>("set_hid_tap_eat", { enabled: !eatEnabled });
      setEatEnabled(next);
      setMsg("");
    } catch (err) {
      setMsg(`切换失败：${err}`);
    } finally {
      setEatBusy(false);
    }
  }

  async function saveTiming(nextLong: number, nextDouble: number) {
    setLongPressMs(nextLong);
    setDoubleClickMs(nextDouble);
    if (!isTauri()) return;
    try {
      await invoke("set_trigger_timing", {
        longPressMs: nextLong,
        doubleClickMs: nextDouble,
      });
      setMsg("");
    } catch (err) {
      setMsg(`保存失败：${err}`);
    }
  }

  // 开关的状态由开关本身表达，不复读一遍。只有「开关说开着、驱动却没就绪」
  // 这种自相矛盾才值得补一句话——那正是按键会失效的情形。
  const tapNotReady = eatEnabled === true && tapStatus !== "attached";

  return (
    <div className="page">
      <div className="section-label">按键信号</div>
      <section className="card form-card">
        <div className="form-row">
          <div className="form-text">
            <div className="form-label">拦截 HID 按键信号</div>
            <p className="hint">
              开启后按键由本应用独占，系统不再响应；关闭则两边同时响应。
            </p>
            {tapNotReady && (
              <p className="hint warn">驱动注入尚未就绪，此时按键可能仍会被系统响应。</p>
            )}
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={eatEnabled === true}
            aria-label="拦截 HID 按键信号"
            className={`switch${eatEnabled ? " on" : ""}`}
            onClick={toggleEat}
            disabled={eatEnabled === null || eatBusy || !isTauri()}
          >
            <span className="switch-thumb" />
          </button>
        </div>
      </section>

      <div className="section-label">按键判定</div>
      <section className="card form-card">
        <div className="form-row">
          <div className="form-text">
            <div className="form-label">长按判定</div>
            <p className="hint">按住多久算长按。麦克风的「按住说话」也用它。</p>
          </div>
          <select
            className="select timing-select"
            aria-label="长按判定"
            value={longPressMs}
            onChange={(e) => saveTiming(Number(e.target.value), doubleClickMs)}
          >
            <option value={400}>0.4 秒</option>
            <option value={550}>0.55 秒</option>
            <option value={700}>0.7 秒</option>
            <option value={1000}>1.0 秒</option>
          </select>
        </div>

        <div className="form-row">
          <div className="form-text">
            <div className="form-label">双击间隔</div>
            <p className="hint">两次按下间隔多短算双击。</p>
          </div>
          <select
            className="select timing-select"
            aria-label="双击间隔"
            value={doubleClickMs}
            onChange={(e) => saveTiming(longPressMs, Number(e.target.value))}
          >
            <option value={200}>0.2 秒</option>
            <option value={300}>0.3 秒</option>
            <option value={400}>0.4 秒</option>
            <option value={500}>0.5 秒</option>
          </select>
        </div>
      </section>

      {msg && <p className="hint">{msg}</p>}
    </div>
  );
}
