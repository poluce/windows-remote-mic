import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppProfileView } from "./types";

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

/** 新建一份配置，返回它的 id。 */
export async function createProfile(
  name: string,
  process: string,
  titleContains: string,
): Promise<string> {
  return invoke<string>("create_app_profile", {
    name,
    process,
    windowTitleContains: titleContains,
  });
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
