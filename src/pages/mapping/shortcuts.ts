import { invoke, isTauri } from "@tauri-apps/api/core";
import { formatComboDisplay, parseComboActionKey, toComboActionKey } from "./combo";

/**
 * 用户命名的自定义快捷键库。
 *
 * **名字只是显示层。** 映射绑定里存的仍然是 `combo:lctrl+k` 这种原始写法，
 * 后端解析、调度器执行、应用专属配置文件全都不认识名字。所以改名或删除一条
 * 命名，已有绑定照样能用，只是显示时回退成原始按键——这也是为什么这个库
 * 读写完**不需要**通知调度器。
 */
export type NamedShortcut = {
  name: string;
  /** 规范化后的组合键 token，例如 `["lctrl", "enter"]`。也是这条记录的身份。 */
  keys: string[];
};

/** 组合键 token 的身份表示：两端都用它比较。 */
export function keysId(keys: string[]): string {
  return keys.join("+");
}

export async function loadShortcuts(): Promise<NamedShortcut[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<NamedShortcut[]>("get_shortcuts");
  } catch (err) {
    void invoke("log_message", { message: `[shortcuts] 读取失败：${err}` }).catch(() => {});
    return [];
  }
}

/** 新增或改名（同一个组合只留一条），返回改完之后的整个库。 */
export async function saveShortcut(
  name: string,
  keys: string[],
): Promise<NamedShortcut[]> {
  return invoke<NamedShortcut[]>("save_shortcut", { name, keys });
}

export async function deleteShortcut(keys: string[]): Promise<NamedShortcut[]> {
  return invoke<NamedShortcut[]>("delete_shortcut", { keys });
}

/** 这个 action_key 对应的自定义名字；不是组合键、或库里没有，返回 null。 */
export function shortcutNameFor(
  library: NamedShortcut[],
  actionKey: string,
): string | null {
  const tokens = parseComboActionKey(actionKey);
  if (!tokens) return null;
  const want = keysId(tokens);
  return library.find((s) => keysId(s.keys) === want)?.name ?? null;
}

/**
 * 组合键在界面上的显示名：库里有名字就用名字，否则退回原始按键。
 *
 * 退回这一支是必要的——用户可能先绑了一个组合键，之后才把名字删掉。
 */
export function comboLabel(
  library: NamedShortcut[],
  keys: string[],
): string {
  if (!keys.length) return "自定义快捷键";
  return shortcutNameFor(library, toComboActionKey(keys)) ?? `快捷键 ${formatComboDisplay(keys)}`;
}
