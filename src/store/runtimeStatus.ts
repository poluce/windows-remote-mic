import { listen } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

export type BridgeStatus = "idle" | "running" | "failed";
export type TapStatus = "idle" | "attached" | "pending" | "unavailable";

export type RuntimeStatusState = {
  connected: boolean;
  bridgeStatus: BridgeStatus;
  tapStatus: TapStatus;
  tapMessage: string;
  endpointsReady: boolean;
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
    }>("get_runtime_status");
    setState({
      connected: snap.connected,
      bridgeStatus: snap.bridge_running ? "running" : "idle",
      endpointsReady: snap.endpoints_ready,
    });
    applyTapEvent(snap.tap_status);
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
}

// 供连接页在 connect 成功后直接更新，无需等事件。
export function markConnected(endpointsReady: boolean) {
  setState({
    connected: true,
    bridgeStatus: "running",
    endpointsReady,
  });
}

export { tapStatusLabel };
