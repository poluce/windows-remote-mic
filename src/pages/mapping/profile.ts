import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppProfileView } from "./types";

/**
 * 浏览器预览用的调色板兜底。
 *
 * 真实来源是后端的 `app_icon_palette`（`core-app-profile/src/model.rs` 的
 * `AUTO_ICON_COLORS`），这里只在没有 Tauri 时兜底，不要拿它当第二事实来源。
 */
const FALLBACK_ICON_PALETTE = [
  "#ff4d4f",
  "#ff7a00",
  "#ffb800",
  "#ffe600",
  "#8ce000",
  "#ff6b8b",
  "#ff2a85",
  "#b84dff",
];

/**
 * 应用专属配置的读写封装。
 *
 * 每个写命令在后端都是「读文件 → 改一处 → 原子写回 → 重载调度器」一整套，
 * 所以这里**不需要**再补一次 reload：顺序一旦交给前端，就会出现「文件写了但
 * 调度器没更新」的窗口，表现成「保存了但按键没反应」。
 */

/** 取回全部应用配置（含没有图标、不进快捷菜单的那些）。 */
export async function loadProfiles(): Promise<AppProfileView[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<AppProfileView[]>("app_profile_catalog");
  } catch (err) {
    // 不静默吞掉：读失败的表现是「作用范围里一份配置都没有」，看起来跟
    // 「用户还没建过配置」一模一样，没有日志就只能靠猜。
    void invoke("log_message", {
      message: `[app-profile] 读取应用配置失败：${err}`,
    }).catch(() => {});
    return [];
  }
}

/** 新建一份配置，返回它的 id。`iconColor` 为空表示内环简标颜色自动生成。 */
export async function createProfile(
  name: string,
  process: string,
  titleContains: string,
  iconColor: string | null = null,
): Promise<string> {
  return invoke<string>("create_app_profile", {
    name,
    process,
    windowTitleContains: titleContains,
    iconColor,
  });
}

/** 修改一份已存在的配置（展示名 / 匹配规则 / 内环颜色）。 */
export async function updateProfile(
  id: string,
  name: string,
  process: string,
  titleContains: string,
  iconColor: string | null,
): Promise<void> {
  await invoke("update_app_profile", {
    id,
    name,
    process,
    windowTitleContains: titleContains,
    iconColor,
  });
}

/**
 * 后端的简标调色板。
 *
 * 不在前端硬编码第二份色卡：内环自动色、颜色选择器、作用范围圆点都从这一处取。
 * 浏览器预览（`isTauri()` 为 false）时退回 `FALLBACK_ICON_PALETTE`。
 */
export async function loadIconPalette(): Promise<string[]> {
  if (!isTauri()) return FALLBACK_ICON_PALETTE;
  try {
    const palette = await invoke<string[]>("app_icon_palette");
    return palette.length ? palette : FALLBACK_ICON_PALETTE;
  } catch {
    return FALLBACK_ICON_PALETTE;
  }
}

/** 把某个 (按键, 触发) 写进这份配置。 */
export async function saveProfileBinding(
  id: string,
  button: string,
  trigger: string,
  action: string,
): Promise<void> {
  await invoke("save_profile_binding", { id, button, trigger, action });
}

/** 「改用全局」：只把这一个格子从这份配置里移掉，其余不动。 */
export async function clearProfileBinding(
  id: string,
  button: string,
  trigger: string,
): Promise<void> {
  await invoke("clear_profile_binding", { id, button, trigger });
}

/** 删掉整份配置，该应用的所有按键回到全局。 */
export async function deleteProfile(id: string): Promise<void> {
  await invoke("delete_app_profile", { id });
}
