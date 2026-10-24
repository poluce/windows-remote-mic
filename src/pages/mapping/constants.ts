/**
 * 映射编辑器目录的**浏览器预览兜底**。
 *
 * 桌面应用里这些数据由后端 `get_mapping_catalog` 单一来源提供（按键、触发方式、
 * 动作分类、默认绑定）；这里保留一份仅用于：
 * 1. 没有 Tauri 后端的浏览器预览；
 * 2. 后端命令异常时的降级显示。
 *
 * 新增动作/按键时以后端为准——如果这里没同步，桌面应用仍然能显示新动作，
 * 只有浏览器预览会缺失。
 */
import type {
  ActionCategory,
  MappingCatalog,
  MappingEntry,
  RemoteButton,
  TriggerOption,
} from "./types";

export const REMOTE_BUTTONS: RemoteButton[] = [
  { key: "power", name: "电源" },
  { key: "mic", name: "麦克风" },
  { key: "up", name: "上" },
  { key: "left", name: "左" },
  { key: "ok", name: "确定" },
  { key: "right", name: "右" },
  { key: "down", name: "下" },
  { key: "back", name: "返回" },
  { key: "volume_up", name: "音量+" },
  { key: "home", name: "主页" },
  { key: "volume_down", name: "音量−" },
  { key: "menu", name: "菜单" },
  { key: "tv", name: "TV" },
];

export const FALLBACK_MAPPING: MappingEntry[] = [
  { button: "power", name: "电源", trigger: "single_click", action: "取消（Esc）", action_key: "escape", enabled: true },
  { button: "up", name: "上", trigger: "single_click", action: "↑", action_key: "arrow_up", enabled: true },
  { button: "down", name: "下", trigger: "single_click", action: "↓", action_key: "arrow_down", enabled: true },
  { button: "left", name: "左", trigger: "single_click", action: "←", action_key: "arrow_left", enabled: true },
  { button: "right", name: "右", trigger: "single_click", action: "→", action_key: "arrow_right", enabled: true },
  { button: "ok", name: "确定", trigger: "single_click", action: "回车（Enter）", action_key: "return", enabled: true },
  { button: "back", name: "返回", trigger: "single_click", action: "删除（退格）", action_key: "delete_backward", enabled: true },
  { button: "home", name: "主页", trigger: "single_click", action: "显示桌面（Win+D）", action_key: "show_desktop", enabled: true },
  { button: "menu", name: "菜单", trigger: "single_click", action: "快捷菜单（开/关）", action_key: "toggle_quick_menu", enabled: true },
  { button: "tv", name: "TV", trigger: "single_click", action: "切换应用（Alt+Tab）", action_key: "app_switcher", enabled: true },
  { button: "volume_up", name: "音量 +", trigger: "single_click", action: "音量 +", action_key: "system_volume_up", enabled: true },
  { button: "volume_down", name: "音量 −", trigger: "single_click", action: "音量 −", action_key: "system_volume_down", enabled: true },
  { button: "mic", name: "麦克风", trigger: "press", action: "语音输入（Win+H）", action_key: "voice", enabled: true },
  { button: "mic", name: "麦克风", trigger: "release", action: "语音输入（Win+H）", action_key: "voice", enabled: true },
];

export const TRIGGERS: TriggerOption[] = [
  { key: "single_click", label: "单击", desc: "按一下立即执行" },
  { key: "double_click", label: "双击", desc: "0.25 秒内按两次" },
  { key: "long_press", label: "长按", desc: "按住约 0.55 秒" },
  { key: "press", label: "按下", desc: "识别为长按后触发（PTT）" },
  { key: "release", label: "松开", desc: "长按结束后触发（PTT）" },
];

/// 麦克风是 PTT 键，只有按下/松开；其它按键只有单击/双击/长按。
export function triggersFor(button: string): TriggerOption[] {
  if (button === "mic") {
    return TRIGGERS.filter((t) => t.key === "press" || t.key === "release");
  }
  return TRIGGERS.filter((t) => t.key !== "press" && t.key !== "release");
}

export const ACTION_CATEGORIES: ActionCategory[] = [
  {
    // 方向、音量 / 播放、语音都并进了这里：它们本来就是「按一个键让系统做件事」，
    // 分成四个分类只是多三次点击。
    //
    // **顺序有讲究**：原来「系统」那几项排在最前不改动。界面上点一个分类会
    // 把动作默认选中该分类的第一项，把方向键挪到前面的话，点「系统」默认就
    // 从「回车」变成「上」了。其余各组按原来分类的先后接在后面，每一组内部
    // 的顺序也照旧——合并前后相对次序不变，肌肉记忆才不会白费。
    key: "system",
    title: "系统",
    actions: [
      { key: "return", label: "回车" },
      { key: "delete_backward", label: "退格" },
      { key: "escape", label: "Esc" },
      { key: "show_desktop", label: "显示桌面" },
      { key: "focus_input", label: "聚焦输入框" },
      { key: "focus_input_or_submit", label: "聚焦输入框 / 回车" },
      { key: "context_menu", label: "右键菜单" },
      { key: "app_switcher", label: "切换应用" },
      { key: "toggle_quick_menu", label: "快捷菜单（开/关）" },
      { key: "arrow_up", label: "上" },
      { key: "arrow_down", label: "下" },
      { key: "arrow_left", label: "左" },
      { key: "arrow_right", label: "右" },
      { key: "system_volume_up", label: "音量 +" },
      { key: "system_volume_down", label: "音量 −" },
      { key: "system_volume_mute", label: "静音" },
      { key: "play_pause", label: "播放 / 暂停" },
      { key: "voice", label: "语音输入（Win+H）" },
    ],
  },
  {
    // 这一栏的按钮不来自下面的 actions，而是**用户在界面上建的快捷键库**
    // （见 mapping/shortcuts.ts）。这里的 `custom_combo` 只是个哨兵值：
    // 选中一个组合键动作时把 category 设成 combo、action 设成它，
    // 真正要保存的 action_key 由 comboTokens 现算。
    key: "combo",
    title: "自定义",
    actions: [{ key: "custom_combo", label: "自定义快捷键" }],
  },
];

export const TRIGGER_LABEL: Record<string, string> = Object.fromEntries(
  TRIGGERS.map((t) => [t.key, t.label])
);
/** 浏览器预览 / 后端异常时的兜底目录（桌面应用以 `get_mapping_catalog` 为准）。 */
export const FALLBACK_CATALOG: MappingCatalog = {
  buttons: REMOTE_BUTTONS.map((button) => ({
    ...button,
    triggers: triggersFor(button.key).map((trigger) => trigger.key),
  })),
  triggers: TRIGGERS,
  categories: ACTION_CATEGORIES,
  default_bindings: FALLBACK_MAPPING,
};
