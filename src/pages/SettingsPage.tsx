import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useRuntimeStatus } from "../store/runtimeStatus";
import { setThemePref, useResolvedTheme, useThemePref, type ThemePref } from "../store/theme";
import {
  checkForUpdates,
  downloadAndInstallUpdate,
  isUpdateBusy,
  useUpdateStatus,
} from "../store/updateStatus";
import "./SettingsPage.css";

const THEME_OPTIONS: { value: ThemePref; label: string }[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const mb = bytes / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.ceil(bytes / 1024)} KB`;
}

function formatClock(ms: number): string {
  if (!ms) return "";
  return new Date(ms).toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  });
}

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
  const [doubleClickMs, setDoubleClickMs] = useState(250);
  const [msg, setMsg] = useState("");
  const { tapStatus } = useRuntimeStatus();
  const themePref = useThemePref();
  const resolvedTheme = useResolvedTheme();
  const update = useUpdateStatus();
  const updateBusy = isUpdateBusy(update.phase);

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
      const saved = await invoke<{
        long_press_ms: number;
        double_click_ms: number;
      }>("set_trigger_timing", {
        longPressMs: nextLong,
        doubleClickMs: nextDouble,
      });
      // 后端会把数值夹到合法区间（200~2000 / 150~1000）：以返回值回写，
      // 避免前端显示超范围数值、与后端实际生效值不一致。
      setLongPressMs(saved.long_press_ms);
      setDoubleClickMs(saved.double_click_ms);
      setMsg("");
    } catch (err) {
      setMsg(`保存失败：${err}`);
    }
  }

  // 开关的状态由开关本身表达，不复读一遍。只有「开关说开着、驱动却没就绪」
  // 这种自相矛盾才值得补一句话——那正是按键会失效的情形。
  const tapNotReady = eatEnabled === true && tapStatus !== "attached";

  // ---- 关于与更新 ----
  const updatePercent =
    update.contentLength > 0
      ? Math.min(
          100,
          Math.round((update.downloadedBytes / update.contentLength) * 100),
        )
      : null;

  const updateHint = (() => {
    if (!isTauri()) return "浏览器预览模式下不可用，请在桌面应用中检查更新。";
    switch (update.phase) {
      case "checking":
        return "正在连接更新服务器…";
      case "up-to-date":
        return `已是最新版本${
          update.lastCheckedAt ? `（上次检查 ${formatClock(update.lastCheckedAt)}）` : ""
        }。`;
      case "available":
        return `发现新版本 v${update.newVersion}。下载安装后应用会自动重启，Windows 会弹出管理员权限确认。`;
      case "downloading": {
        const total =
          update.contentLength > 0 ? ` / ${formatBytes(update.contentLength)}` : "";
        return `正在下载更新… ${formatBytes(update.downloadedBytes)}${total}`;
      }
      case "installing":
        return "正在安装更新，应用即将自动重启…";
      case "error":
        return update.error || "检查更新失败，请稍后重试。";
      default:
        return "检查 GitHub Releases 上的新版本，下载、校验签名并自动安装。正式版启动后及运行期间每小时最多自动检查一次，也可以随时手动检查。";
    }
  })();

  const updateButtonLabel = (() => {
    switch (update.phase) {
      case "checking":
        return "检查中…";
      case "available":
        return "下载并安装";
      case "downloading":
        return updatePercent === null ? "下载中…" : `下载中 ${updatePercent}%`;
      case "installing":
        return "安装中…";
      case "up-to-date":
        return "重新检查";
      case "error":
        return "重试";
      default:
        return "检查更新";
    }
  })();

  async function onUpdateClick() {
    if (update.phase === "available") {
      await downloadAndInstallUpdate();
    } else {
      await checkForUpdates();
    }
  }

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <h1>通用设置</h1>
          <p>按键拦截、长按与双击判定，以及关于与更新。</p>
        </div>
      </div>

      <div className="section-label">外观</div>
      <section className="card form-card">
        <div className="form-row">
          <div className="form-text">
            <div className="form-label">主题</div>
            <p className="hint">
              跟随系统时会随 Windows 的浅色 / 深色设置自动切换
              {themePref === "system" && `（当前为${resolvedTheme === "dark" ? "深色" : "浅色"}）`}。
            </p>
          </div>
          <div className="theme-options" role="group" aria-label="主题">
            {THEME_OPTIONS.map((opt) => (
              <button
                key={opt.value}
                type="button"
                className={`btn small${themePref === opt.value ? " on" : ""}`}
                aria-pressed={themePref === opt.value}
                onClick={() => setThemePref(opt.value)}
              >
                {opt.label}
              </button>
            ))}
          </div>
        </div>
      </section>

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
            <p className="hint">
              两次按下间隔多短算双击。只有绑定了双击的按键才会等这个窗口；没绑双击的键，单击松开立即生效。
            </p>
          </div>
          <select
            className="select timing-select"
            aria-label="双击间隔"
            value={doubleClickMs}
            onChange={(e) => saveTiming(longPressMs, Number(e.target.value))}
          >
            <option value={200}>0.2 秒</option>
            <option value={250}>0.25 秒</option>
            <option value={300}>0.3 秒</option>
            <option value={400}>0.4 秒</option>
            <option value={500}>0.5 秒</option>
          </select>
        </div>
      </section>

      <div className="section-label">关于与更新</div>
      <section className="card form-card">
        <div className="form-row">
          <div className="form-text">
            <div className="form-label">
              Remote Mic{update.currentVersion ? ` v${update.currentVersion}` : ""}
            </div>
            <p className={`hint${update.phase === "error" ? " warn" : ""}`}>{updateHint}</p>
            {update.phase === "downloading" && (
              <div
                className="update-progress"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={updatePercent ?? undefined}
              >
                <span
                  style={{
                    width: updatePercent === null ? "100%" : `${updatePercent}%`,
                    opacity: updatePercent === null ? 0.35 : 1,
                  }}
                />
              </div>
            )}
          </div>
          <button
            type="button"
            className="btn primary small"
            onClick={onUpdateClick}
            disabled={updateBusy || !isTauri()}
          >
            {updateButtonLabel}
          </button>
        </div>
        {update.phase === "available" && update.notes && (
          <div className="form-row">
            <div className="form-text">
              <div className="form-label">更新说明</div>
              <p className="hint update-notes">{update.notes}</p>
            </div>
          </div>
        )}
      </section>

      {msg && <p className="hint">{msg}</p>}
    </div>
  );
}
