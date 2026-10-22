import { invoke, isTauri } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useSyncExternalStore } from "react";

/**
 * 应用更新状态的单一事实来源。
 *
 * 与 `runtimeStatus.ts` 同一套模式：模块级状态 + useSyncExternalStore，
 * 页面切换不会丢进度，侧栏也能拿到「有新版本」做提示。
 *
 * 更新链路（Tauri v2 updater）：
 *   GitHub Releases 上的 `latest.json`（由发布工作流生成）
 *     -> check() 比较版本
 *     -> downloadAndInstall() 下载带签名的安装包、校验签名后拉起 NSIS 安装器
 *     -> Windows 上安装器会自动关闭本应用、完成安装并重启新版本。
 *
 * 因此这里**不需要**自己调用 restart：`downloadAndInstall` 成功时应用即退出。
 *
 * 检查时机与频率（仅正式构建；手动检查任何时候都可点）：
 *   - 启动后立即异步检查一次（不延迟、不阻塞窗口）；
 *   - 运行期间每 30 分钟复查一次；
 *   - 启动检查 / 定时复查都要求距上次成功检查 ≥1 小时才真正发请求；
 *   - 上次成功检查时间持久化在 `config.json`（`last_update_check_at`），重启后依然生效。
 */
export type UpdatePhase =
  | "idle"
  | "checking"
  | "up-to-date"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export type UpdateState = {
  phase: UpdatePhase;
  /** 当前运行版本（后端 `app_version` 命令返回，与更新比较同源）。 */
  currentVersion: string;
  /** 远端发现的新版本号；无更新时为空。 */
  newVersion: string;
  /** 远端附带的发布说明（`latest.json` 的 notes）。 */
  notes: string;
  downloadedBytes: number;
  /** 安装包总字节数；服务端未给 Content-Length 时为 0。 */
  contentLength: number;
  error: string;
  /** 上次检查完成的时间（毫秒时间戳），0 表示还没检查过。 */
  lastCheckedAt: number;
};

const initial: UpdateState = {
  phase: "idle",
  currentVersion: "",
  newVersion: "",
  notes: "",
  downloadedBytes: 0,
  contentLength: 0,
  error: "",
  lastCheckedAt: 0,
};

let state: UpdateState = initial;
/** 已发现但还没安装的更新；Tauri 的 Update 是资源对象，需要显式 close。 */
let pendingUpdate: Update | null = null;
let initialized = false;
let inflight: Promise<void> | null = null;
const listeners = new Set<() => void>();

function emitChange() {
  for (const listener of listeners) {
    listener();
  }
}

function setState(patch: Partial<UpdateState>) {
  state = { ...state, ...patch };
  emitChange();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getUpdateStatus(): UpdateState {
  return state;
}

export function useUpdateStatus(): UpdateState {
  return useSyncExternalStore(subscribe, getUpdateStatus);
}

/** 检查中或下载安装中：按钮应禁用，避免并发触发。 */
export function isUpdateBusy(phase: UpdatePhase): boolean {
  return phase === "checking" || phase === "downloading" || phase === "installing";
}

/** 自动检查的最小联网间隔：距上次成功检查不足 1 小时就不发请求。 */
const AUTO_CHECK_MIN_INTERVAL_MS = 60 * 60 * 1000;
/** 运行期间复查定时器：每 30 分钟唤醒一次，实际联网仍受上面的 1 小时门槛限制。 */
const AUTO_CHECK_TIMER_MS = 30 * 60 * 1000;

/** 自动检查（启动 / 定时）是否已过「距上次成功检查 ≥1 小时」的门槛；手动检查不走这里。 */
function autoCheckDue(): boolean {
  return Date.now() - state.lastCheckedAt >= AUTO_CHECK_MIN_INTERVAL_MS;
}

/** 记录一次成功的检查（有更新 / 无更新都算），并持久化时间戳供重启后节流。 */
function recordCheckSuccess(patch: Partial<UpdateState>) {
  const now = Date.now();
  setState({ ...patch, lastCheckedAt: now });
  void invoke("set_last_update_check_at", {
    timestamp: Math.floor(now / 1000),
  }).catch(() => {});
}

async function closePendingUpdate() {
  const update = pendingUpdate;
  pendingUpdate = null;
  if (!update) return;
  try {
    await update.close();
  } catch {
    // 资源可能已被后端释放（例如安装成功后应用退出），忽略即可。
  }
}

async function runCheck(options: { silent: boolean }): Promise<void> {
  if (!isTauri()) {
    setState({ phase: "error", error: "检查更新需要在桌面应用中进行。" });
    return;
  }
  if (isUpdateBusy(state.phase)) return;

  await closePendingUpdate();
  setState({
    phase: "checking",
    newVersion: "",
    notes: "",
    error: "",
    downloadedBytes: 0,
    contentLength: 0,
  });

  try {
    const update = await check({ timeout: 30_000 });
    if (!update) {
      recordCheckSuccess({ phase: "up-to-date" });
      return;
    }
    pendingUpdate = update;
    recordCheckSuccess({
      phase: "available",
      newVersion: update.version,
      notes: update.body ?? "",
    });
  } catch (err) {
    // 启动时的静默检查失败不打扰用户（离线、代理、旧版本还没有 latest.json 都很常见）；
    // 手动检查时把原因展示出来。
    if (options.silent) {
      setState({ phase: "idle" });
    } else {
      setState({ phase: "error", error: friendlyCheckError(err) });
    }
  }
}

function friendlyCheckError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  if (/timed? ?out|timeout/i.test(raw)) {
    return "检查更新超时，请检查网络后重试。";
  }
  if (/404|not found|release not found/i.test(raw)) {
    return "更新服务暂时不可用（暂未找到更新信息），请稍后重试或前往 Releases 页面手动下载。";
  }
  return raw;
}

function friendlyDownloadError(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  if (/signature|verif/i.test(raw)) {
    return "更新包签名校验失败，已取消安装。请前往 Releases 页面手动下载。";
  }
  if (/timed? ?out|timeout/i.test(raw)) {
    return "下载更新包超时，请检查网络后重试。";
  }
  if (/denied|elevat|uac|os error 5/i.test(raw)) {
    return "安装更新被系统拒绝（可能取消了管理员权限确认），请重试并允许提权。";
  }
  if (/404|not found|download|http/i.test(raw)) {
    return "下载更新包失败（安装包不存在或网络异常），请稍后重试或前往 Releases 页面手动下载。";
  }
  return raw;
}

/**
 * 手动检查更新。
 *
 * 启动时的自动检查由 {@link initUpdateStatus} 负责（仅正式构建，静默失败）。
 */
export async function checkForUpdates(): Promise<void> {
  if (inflight) return inflight;
  inflight = runCheck({ silent: false }).finally(() => {
    inflight = null;
  });
  return inflight;
}

/**
 * 下载并安装已发现的更新。
 *
 * Windows 上安装器会关闭并重启本应用，所以函数返回后前端通常立刻随进程退出，
 * 不需要再手动 relaunch。
 */
export async function downloadAndInstallUpdate(): Promise<void> {
  if (inflight) return inflight;
  if (!pendingUpdate || state.phase !== "available") return;
  const update = pendingUpdate;
  inflight = (async () => {
    setState({
      phase: "downloading",
      downloadedBytes: 0,
      contentLength: 0,
      error: "",
    });
    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            setState({ contentLength: event.data.contentLength ?? 0 });
            break;
          case "Progress":
            setState({
              downloadedBytes: state.downloadedBytes + event.data.chunkLength,
            });
            break;
          case "Finished":
            setState({ phase: "installing" });
            // 安装器拉起后进程会被直接结束（updater 在 Windows 上是 exit(0)），
            // 提前让后端放开所有按键，避免虚拟 HID 残留按住状态。
            void invoke("prepare_for_update_exit").catch(() => {});
            break;
        }
      });
      // Windows：安装器已被拉起，应用即将退出；macOS/Linux 才需要 relaunch。
      // 本应用只发布 Windows，此处不再引入 process 插件。
      setState({ phase: "installing" });
    } catch (err) {
      pendingUpdate = update;
      setState({ phase: "error", error: friendlyDownloadError(err) });
    }
  })().finally(() => {
    inflight = null;
  });
  return inflight;
}

/**
 * 初始化：读取当前版本与上次成功检查时间，然后启动自动检查。
 *
 * 自动检查策略（仅正式构建）：
 *   - 启动后**立即**异步检查一次，不延迟、不阻塞窗口；
 *   - 运行期间每 30 分钟复查一次；
 *   - 启动检查与定时复查都要求距上次成功检查 ≥1 小时才真正联网，否则直接跳过；
 *   - 手动「检查更新」不受频率限制。
 *
 * 幂等：App 的 effect 在严格模式 / HMR 下可能触发多次。
 */
export function initUpdateStatus(): void {
  if (initialized) return;
  initialized = true;

  if (!isTauri()) return;
  invoke<string>("app_version")
    .then((version) => setState({ currentVersion: version }))
    .catch(() => {});

  // 先读回「上次成功检查」时间，再决定启动时要不要真的联网。
  invoke<number>("get_last_update_check_at")
    .then((timestamp) => {
      if (timestamp > 0) setState({ lastCheckedAt: timestamp * 1000 });
    })
    .catch(() => {})
    .finally(() => {
      // 开发模式（vite dev）下不自动检查：避免每次热更新都打一次 GitHub，
      // 也给「检查更新」按钮保留一个可复现的手动入口。
      if (!import.meta.env.PROD) return;
      startAutoUpdateChecks();
    });
}

/** 启动自动检查：启动立即查一次 + 运行期间每 30 分钟复查（均受 1 小时门槛限制）。 */
function startAutoUpdateChecks() {
  if (autoCheckDue()) {
    void runCheck({ silent: true }).catch(() => {});
  }
  window.setInterval(() => {
    if (autoCheckDue() && !isUpdateBusy(state.phase)) {
      void runCheck({ silent: true }).catch(() => {});
    }
  }, AUTO_CHECK_TIMER_MS);
}
