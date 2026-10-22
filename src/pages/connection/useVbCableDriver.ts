import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

/** 虚拟声卡下拉框的就绪状态。 */
export type DriverStatus = "loading" | "ready" | "missing" | "unknown";

/** 虚拟声卡选项（目前只实现 VB-CABLE，其余为占位）。 */
export const DRIVER_OPTIONS = [
  { value: "vb_cable", label: "VB-CABLE", disabled: false },
  { value: "voicemeeter", label: "Voicemeeter（预留）", disabled: true },
  { value: "rearoute", label: "ReaRoute（预留）", disabled: true },
] as const;

type VbCableStatus = {
  input: boolean;
  output: boolean;
  ready: boolean;
};

/**
 * 连接页「语音链路 → 虚拟声卡」下拉框的本地交互状态。
 *
 * - 挂载时查询一次 VB-CABLE 是否就绪；
 * - 下拉框点击外部自动收起（事件监听在组件卸载时清理）。
 */
export function useVbCableDriver() {
  const [virtualDriver, setVirtualDriver] = useState("vb_cable");
  const [driverStatus, setDriverStatus] = useState<DriverStatus>("unknown");
  const [driverOpen, setDriverOpen] = useState(false);
  const driverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    if (!isTauri()) {
      setDriverStatus("unknown");
      return () => {
        cancelled = true;
      };
    }
    setDriverStatus("loading");
    invoke<VbCableStatus>("vb_cable_status")
      .then((s) => {
        if (!cancelled) setDriverStatus(s.ready ? "ready" : "missing");
      })
      .catch(() => {
        if (!cancelled) setDriverStatus("missing");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    function onPointerDown(e: MouseEvent) {
      if (driverRef.current && !driverRef.current.contains(e.target as Node)) {
        setDriverOpen(false);
      }
    }
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, []);

  return {
    virtualDriver,
    setVirtualDriver,
    driverStatus,
    driverOpen,
    setDriverOpen,
    driverRef,
  };
}
