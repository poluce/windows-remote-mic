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
      setState({ phase: "up-to-date", lastCheckedAt: Date.now() });
      return;
    }
    pendingUpdate = update;
    setState({
      phase: "available",
      newVersion: update.version,
      notes: update.body ?? "",
      lastCheckedAt: Date.now(),
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
 * 初始化：读取当前版本；正式构建下延迟做一次静默检查。
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

  // 开发模式（vite dev）下不自动检查：避免每次热更新都打一次 GitHub API，
  // 也给「检查更新」按钮保留一个可复现的手动入口。
  if (import.meta.env.PROD) {
    window.setTimeout(() => {
      if (state.phase === "idle") {
        void runCheck({ silent: true }).catch(() => {});
      }
    }, 6000);
  }
}
