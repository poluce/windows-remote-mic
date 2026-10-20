import { listen } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

export type BridgeStatus = "idle" | "running" | "failed";
export type TapStatus = "idle" | "attached" | "pending" | "unavailable";

/** 后端语音桥的细粒度状态（重连进度 / 失败原因）。 */
export type BridgeState =
  | "idle"
  | "starting"
  | "running"
  | "retrying"
  | "waiting"
  | "failed"
  | "stopped";

export type BridgeStatusSnapshot = {
  state: BridgeState;
  attempt: number;
  failures: number;
  next_retry_secs: number;
  last_error: string;
};

export type RuntimeStatusState = {
  connected: boolean;
  bridgeStatus: BridgeStatus;
  tapStatus: TapStatus;
  tapMessage: string;
  endpointsReady: boolean;
  /** 语音桥重连进度（来自后端 bridge-status 事件）。 */
  bridgeState: BridgeState;
  bridgeAttempt: number;
  bridgeFailures: number;
  bridgeNextRetrySecs: number;
  bridgeLastError: string;
};

type TapStatusEvent = {
  state: TapStatus;
  message: string;
};

const initial: RuntimeStatusState = {
  connected: false,
  bridgeStatus: "idle",
  tapStatus: "idle",
  tapMessage: "",
  endpointsReady: false,
  bridgeState: "idle",
  bridgeAttempt: 0,
  bridgeFailures: 0,
  bridgeNextRetrySecs: 0,
  bridgeLastError: "",
};

let state: RuntimeStatusState = initial;
const listeners = new Set<() => void>();

function emitChange() {
  for (const listener of listeners) {
    listener();
  }
}

function setState(patch: Partial<RuntimeStatusState>) {
  state = { ...state, ...patch };
  emitChange();
}

export function getRuntimeStatus(): RuntimeStatusState {
  return state;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useRuntimeStatus(): RuntimeStatusState {
  return useSyncExternalStore(subscribe, getRuntimeStatus);
}

function applyTapEvent(evt: TapStatusEvent | null | undefined) {
  if (!evt) {
    setState({ tapStatus: "idle", tapMessage: "" });
    return;
  }
  setState({
    tapStatus: evt.state,
    tapMessage: evt.message,
  });
}

function applyBridgeSnapshot(evt: BridgeStatusSnapshot | null | undefined) {
  if (!evt) {
    setState({
      bridgeState: "idle",
      bridgeAttempt: 0,
      bridgeFailures: 0,
      bridgeNextRetrySecs: 0,
      bridgeLastError: "",
    });
    return;
  }
  setState({
    bridgeState: evt.state,
    bridgeAttempt: evt.attempt,
    bridgeFailures: evt.failures,
    bridgeNextRetrySecs: evt.next_retry_secs,
    bridgeLastError: evt.last_error,
    // 让旧字段与新状态保持一致：running→running，其余非 idle 都算异常。
    bridgeStatus:
      evt.state === "running"
        ? "running"
        : evt.state === "failed"
          ? "failed"
          : "idle",
  });
}

function tapStatusLabel(status: TapStatus): string {
  switch (status) {
    case "attached":
      return "返回/音量旁路已附着";
    case "pending":
      return "返回/音量旁路重连中";
    case "unavailable":
      return "返回/音量旁路未启用";
    default:
      return "返回/音量旁路";
  }
}

// 全局只初始化一次事件监听；不依赖任何页面是否挂载。
let initialized = false;
export async function initRuntimeStatus() {
  if (initialized || !isTauri()) return;
  initialized = true;

  // 先拉一次后端快照，恢复上次连接后的状态。
  try {
    const snap = await invoke<{
      connected: boolean;
      bridge_running: boolean;
      tap_status: TapStatusEvent | null;
      endpoints_ready: boolean;
      bridge_status: BridgeStatusSnapshot | null;
    }>("get_runtime_status");
    setState({
      connected: snap.connected,
      bridgeStatus: snap.bridge_running ? "running" : "idle",
      endpointsReady: snap.endpoints_ready,
    });
    applyTapEvent(snap.tap_status);
    applyBridgeSnapshot(snap.bridge_status);
  } catch {
    // 忽略：后端暂不可用时保持默认
  }

  await listen<boolean>("ble-connection-status", (event) => {
    setState({
      connected: event.payload,
      bridgeStatus: event.payload ? "running" : "idle",
    });
  });

  await listen<TapStatusEvent>("hid-tap-status", (event) => {
    applyTapEvent(event.payload);
  });

  await listen<BridgeStatusSnapshot>("bridge-status", (event) => {
    applyBridgeSnapshot(event.payload);
  });
}

// 供连接页在 connect 成功后直接更新，无需等事件。
//
// 注意：`connect_rc003` 可能命中 GATT 缓存，即使链路未连接也能发现端点。
// 因此 connected 必须以「真实链路状态」linkConnected 为准，否则会出现
// 「已连接 + 等待遥控器上线」这类自相矛盾的 UI。
export function markConnected(endpointsReady: boolean, linkConnected: boolean) {
  setState({
    connected: linkConnected,
    bridgeStatus: linkConnected ? "running" : "idle",
    endpointsReady: linkConnected ? endpointsReady : false,
  });
}

export function markDisconnected() {
  setState({
    connected: false,
    bridgeStatus: "idle",
    endpointsReady: false,
    bridgeState: "stopped",
    bridgeAttempt: 0,
    bridgeFailures: 0,
    bridgeNextRetrySecs: 0,
    bridgeLastError: "",
  });
}

/** 语音桥状态的人类可读标签（连接页状态胶囊用）。 */
export function bridgeStateLabel(state: BridgeState, attempt: number): string {
  switch (state) {
    case "starting":
      return "语音桥启动中…";
    case "running":
      return "ATVV 语音桥";
    case "retrying":
      return `语音桥重试中（第 ${attempt} 次）`;
    case "waiting":
      return "等待遥控器上线";
    case "failed":
      return "语音桥连接失败";
    case "stopped":
      return "语音桥已停止";
    default:
      return "ATVV 语音桥";
  }
}

export { tapStatusLabel };
