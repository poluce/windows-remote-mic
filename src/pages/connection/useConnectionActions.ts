import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { OUTPUT_DEVICE } from "../../constants";
import {
  markConnected,
  markDisconnected,
  useEffectiveConnected,
} from "../../store/runtimeStatus";

/** `stop_voice_bridge` 的返回：是否真的停下 + 提示文案。 */
export type BridgeStopResult = {
  stopped: boolean;
  message: string;
};

type Rc003Device = {
  id: string;
  name: string;
};

type AtvvEndpoints = {
  tx?: string;
  audio?: string;
  control?: string;
};

type Rc003Connection = {
  device: Rc003Device;
  endpoints: AtvvEndpoints;
  /** 真实 BLE 链路状态（GATT 缓存发现成功 ≠ 链路已连接）。 */
  link_connected: boolean;
};

/**
 * 连接页的设备动作（扫描 / 连接 / 断开 / 重试）与自动连接逻辑。
 *
 * 从页面组件抽出来，页面只负责渲染；副作用与后端命令调用集中在这里，
 * 便于单独测试/复用（例如以后做托盘菜单的「连接」）。
 */
export function useConnectionActions() {
  const effectiveConnected = useEffectiveConnected();
  const [scanning, setScanning] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [feedback, setFeedback] = useState("");

  const scan = useCallback(async () => {
    if (!isTauri()) {
      setFeedback("请在桌面应用内扫描");
      return;
    }
    setScanning(true);
    setFeedback("正在扫描蓝牙…（请确认遥控器已在 Windows 蓝牙中配对）");
    try {
      const device = await invoke<Rc003Device>("scan_for_rc003");
      setFeedback(`扫描成功：${device.name}`);
    } catch (err) {
      setFeedback(`扫描失败：${err}`);
    } finally {
      setScanning(false);
    }
  }, []);

  const connect = useCallback(async (options?: { auto?: boolean }) => {
    if (!isTauri()) {
      setFeedback("请在桌面应用内连接");
      return;
    }
    setConnecting(true);
    setFeedback(
      options?.auto
        ? "检测到上次使用的遥控器，正在自动连接…"
        : "正在连接并枚举 GATT 特征…",
    );
    try {
      const result = await invoke<Rc003Connection>("connect_rc003");
      const endpointsReady = Boolean(result.endpoints.audio && result.endpoints.control);
      markConnected(endpointsReady, result.link_connected);
      setFeedback("");
      try {
        await invoke("save_selected_device", { deviceId: result.device.id });
      } catch {
        // 忽略保存错误
      }

      // 连接成功后，自动启动后台语音桥
      try {
        await invoke<string>("start_voice_bridge", {
          deviceId: result.device.id,
          outputDevice: OUTPUT_DEVICE,
        });
      } catch (bridgeErr) {
        setFeedback(`语音桥启动失败：${bridgeErr}`);
      }
    } catch (err) {
      // 设备当前不可达（休眠 / 被另一台电脑占用 / 蓝牙未开）：
      // 只要本机还留着配对信息，就进入「等待遥控器上线」模式——
      // 监督线程只监听链路事件，系统连上后会自动恢复语音，不再定时盲试。
      const savedId = await invoke<string | null>("get_saved_device_id").catch(
        () => null,
      );
      if (savedId) {
        try {
          await invoke<string>("start_voice_bridge", {
            deviceId: savedId,
            outputDevice: OUTPUT_DEVICE,
          });
          setFeedback(
            options?.auto
              ? "遥控器当前不在线，已进入等待（系统连上后自动恢复）"
              : "遥控器当前不在线，已进入等待：请唤醒遥控器，系统连接后会自动恢复语音",
          );
          return;
        } catch (bridgeErr) {
          setFeedback(`语音桥启动失败：${bridgeErr}`);
          return;
        }
      }
      setFeedback(
        options?.auto
          ? "自动连接未成功，请唤醒遥控器后点「连接」重试"
          : `连接失败：${err}`,
      );
    } finally {
      setConnecting(false);
    }
  }, []);

  /** 失败状态下重试：先停旧监督线程，再重新启动。 */
  const retryBridge = useCallback(async () => {
    const savedId = await invoke<string | null>("get_saved_device_id").catch(
      () => null,
    );
    if (!savedId) {
      setFeedback("没有已保存的遥控器，请先重新配对后再连接");
      return;
    }
    setConnecting(true);
    setFeedback("正在重试连接…");
    try {
      const message = await invoke<string>("retry_voice_bridge", {
        deviceId: savedId,
        outputDevice: OUTPUT_DEVICE,
      });
      setFeedback(message);
    } catch (err) {
      setFeedback(`重试失败：${err}`);
    } finally {
      setConnecting(false);
    }
  }, []);

  const disconnect = useCallback(async () => {
    if (!isTauri()) {
      markDisconnected();
      return;
    }
    setDisconnecting(true);
    setFeedback("");
    try {
      const result = await invoke<BridgeStopResult>("stop_voice_bridge");
      // 后端没真正停下时不要假装断开，否则用户会以为已经停止而实际仍在重连。
      if (result.stopped) {
        markDisconnected();
      }
      setFeedback(result.message);
    } catch (err) {
      setFeedback(`断开失败：${err}`);
    } finally {
      setDisconnecting(false);
    }
  }, []);

  // 启动时自动尝试连接「上次成功连接的设备」，省掉每次开机手动点连接。
  // 仅挂载时尝试一次；失败保持安静（手动点连接会给出明确错误）。
  // 注意：React StrictMode 会「挂载→卸载→再挂载」跑两次 effect，
  // 这里不设 cancelled 短路，否则第一次的异步请求会被取消、第二次又被
  // autoConnectTried 拦住，导致自动连接永远不生效（开发模式实测）。
  const autoConnectTried = useRef(false);
  useEffect(() => {
    if (autoConnectTried.current || !isTauri() || effectiveConnected || connecting) {
      return;
    }
    autoConnectTried.current = true;
    void (async () => {
      try {
        const savedId = await invoke<string | null>("get_saved_device_id");
        void invoke("log_message", {
          message: `[auto-connect] 启动自动连接：saved_device_id=${savedId ?? "null"}`,
        }).catch(() => {});
        if (!savedId) {
          return;
        }
        await connect({ auto: true });
      } catch (err) {
        // 忽略：自动连接只是锦上添花，不打扰用户。
        void invoke("log_message", {
          message: `[auto-connect] 自动连接异常：${String(err)}`,
        }).catch(() => {});
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return {
    scanning,
    connecting,
    disconnecting,
    feedback,
    scan,
    connect,
    retryBridge,
    disconnect,
  };
}
