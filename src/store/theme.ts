import { useSyncExternalStore } from "react";

/**
 * 主题偏好（浅色 / 深色 / 跟随系统）的单一事实来源。
 *
 * 与 `runtimeStatus` / `updateStatus` 同一套模式：模块级状态 + useSyncExternalStore，
 * 页面切换不会丢偏好。
 *
 * 为什么不进 config.json：这是纯前端的显示偏好，后端不需要知道，也不会跨设备同步；
 * 用 localStorage 存即可（与快捷菜单的菜单停留位置同一类数据）。
 *
 * 生效方式：`<html data-theme="light|dark">`，深色样式全部由 tokens.css 的
 * `:root[data-theme="dark"]` 覆盖，组件 CSS 不需要知道当前是什么主题。
 */
export type ThemePref = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "remote-mic-theme";
const DARK_QUERY = "(prefers-color-scheme: dark)";

const listeners = new Set<() => void>();

function readPref(): ThemePref {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (value === "light" || value === "dark" || value === "system") {
      return value;
    }
  } catch {
    // localStorage 不可用（隐私模式 / 沙箱）：按跟随系统处理。
  }
  return "system";
}

/** 是否支持 prefers-color-scheme；不支持的旧内核一律按浅色。 */
function systemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "light";
  return window.matchMedia(DARK_QUERY).matches ? "dark" : "light";
}

function resolveTheme(value: ThemePref): ResolvedTheme {
  return value === "system" ? systemTheme() : value;
}

let pref: ThemePref = readPref();
let resolved: ResolvedTheme = resolveTheme(pref);

function applyTheme() {
  resolved = resolveTheme(pref);
  document.documentElement.dataset.theme = resolved;
}

function notify() {
  listeners.forEach((fn) => fn());
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getThemePref(): ThemePref {
  return pref;
}

export function getResolvedTheme(): ResolvedTheme {
  return resolved;
}

export function setThemePref(value: ThemePref) {
  if (value === pref) return;
  pref = value;
  try {
    localStorage.setItem(STORAGE_KEY, value);
  } catch {
    // 存不了就只在本次会话生效。
  }
  applyTheme();
  notify();
}

/**
 * 应用启动时调用一次：立刻应用当前主题，并监听系统主题变化
 * （只有「跟随系统」时系统变化才会带来视觉变化）。
 */
export function initTheme() {
  applyTheme();

  if (typeof window !== "undefined" && window.matchMedia) {
    const media = window.matchMedia(DARK_QUERY);
    const onChange = () => {
      if (pref !== "system") return;
      const next = systemTheme();
      if (next === resolved) return;
      applyTheme();
      notify();
    };
    if (media.addEventListener) {
      media.addEventListener("change", onChange);
    } else {
      // 老内核只有弃用接口，WebView2 不会走到这里，兜底而已。
      media.addListener(onChange);
    }
  }

  notify();
}

export function useThemePref(): ThemePref {
  return useSyncExternalStore(subscribe, getThemePref, getThemePref);
}

export function useResolvedTheme(): ResolvedTheme {
  return useSyncExternalStore(subscribe, getResolvedTheme, getResolvedTheme);
}
