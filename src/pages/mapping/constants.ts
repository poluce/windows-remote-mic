import type {
  ActionCategory,
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
  { button: "power", name: "电源", trigger: "single_click", action: "取消（Esc）", action_key: "escape" },
  { button: "up", name: "上", trigger: "single_click", action: "↑", action_key: "arrow_up" },
  { button: "down", name: "下", trigger: "single_click", action: "↓", action_key: "arrow_down" },
  { button: "left", name: "左", trigger: "single_click", action: "←", action_key: "arrow_left" },
  { button: "right", name: "右", trigger: "single_click", action: "→", action_key: "arrow_right" },
  { button: "ok", name: "确定", trigger: "single_click", action: "回车（Enter）", action_key: "return" },
  { button: "back", name: "返回", trigger: "single_click", action: "删除（退格）", action_key: "delete_backward" },
  { button: "home", name: "主页", trigger: "single_click", action: "显示桌面（Win+D）", action_key: "show_desktop" },
  { button: "menu", name: "菜单", trigger: "single_click", action: "右键菜单（上下文菜单）", action_key: "context_menu" },
  { button: "tv", name: "TV", trigger: "single_click", action: "切换应用（Alt+Tab）", action_key: "app_switcher" },
  { button: "volume_up", name: "音量 +", trigger: "single_click", action: "音量 +", action_key: "system_volume_up" },
  { button: "volume_down", name: "音量 −", trigger: "single_click", action: "音量 −", action_key: "system_volume_down" },
  { button: "mic", name: "麦克风", trigger: "press", action: "语音输入（Win+H）", action_key: "voice" },
  { button: "mic", name: "麦克风", trigger: "release", action: "语音输入（Win+H）", action_key: "voice" },
];

export const TRIGGERS: TriggerOption[] = [
  { key: "single_click", label: "单击", desc: "按一下立即执行" },
  { key: "double_click", label: "双击", desc: "0.3 秒内按两次" },
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
    key: "dir",
    title: "方向",
    actions: [
      { key: "arrow_up", label: "上" },
      { key: "arrow_down", label: "下" },
      { key: "arrow_left", label: "左" },
      { key: "arrow_right", label: "右" },
    ],
  },
  {
    key: "system",
    title: "系统",
    actions: [
      { key: "return", label: "回车" },
      { key: "delete_backward", label: "退格" },
      { key: "escape", label: "Esc" },
      { key: "show_desktop", label: "显示桌面" },
      { key: "context_menu", label: "右键菜单" },
      { key: "app_switcher", label: "切换应用" },
    ],
  },
  {
    key: "media",
    title: "音量 / 播放",
    actions: [
      { key: "system_volume_up", label: "音量 +" },
      { key: "system_volume_down", label: "音量 −" },
      { key: "system_volume_mute", label: "静音" },
      { key: "play_pause", label: "播放 / 暂停" },
    ],
  },
  {
    key: "voice",
    title: "语音",
    actions: [{ key: "voice", label: "语音输入（Win+H）" }],
  },
  {
    key: "other",
    title: "其他",
    actions: [{ key: "disabled", label: "禁用" }],
  },
];

export const TRIGGER_LABEL: Record<string, string> = Object.fromEntries(
  TRIGGERS.map((t) => [t.key, t.label])
);