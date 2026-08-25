import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";

type MappingEntry = {
  button: string;
  name: string;
  action: string;
};

const REMOTE_BUTTONS = [
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

const FALLBACK_MAPPING: MappingEntry[] = [
  { button: "power", name: "电源", action: "取消（Esc）" },
  { button: "up", name: "上", action: "↑" },
  { button: "down", name: "下", action: "↓" },
  { button: "left", name: "左", action: "←" },
  { button: "right", name: "右", action: "→" },
  { button: "ok", name: "确定", action: "回车（Enter）" },
  { button: "back", name: "返回", action: "删除（退格）" },
  { button: "home", name: "主页", action: "显示桌面（Win+D）" },
  { button: "menu", name: "菜单", action: "右键菜单（上下文菜单）" },
  { button: "tv", name: "TV", action: "切换应用（Alt+Tab）" },
  { button: "volume_up", name: "音量 +", action: "音量 +" },
  { button: "volume_down", name: "音量 −", action: "音量 −" },
  { button: "mic", name: "麦克风", action: "语音输入（Win+H）" },
];

const TRIGGERS: { key: string; label: string; desc: string }[] = [
  { key: "single_click", label: "单击", desc: "按一下立即执行" },
  { key: "double_click", label: "双击", desc: "0.3 秒内按两次" },
  { key: "long_press", label: "长按", desc: "按住约 0.55 秒" },
];

const ACTION_CATEGORIES: { key: string; title: string; actions: { key: string; label: string }[] }[] = [
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

const TRIGGER_LABEL: Record<string, string> = Object.fromEntries(
  TRIGGERS.map((t) => [t.key, t.label])
);

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [category, setCategory] = useState("system");
  const [action, setAction] = useState("return");
  const [saveMsg, setSaveMsg] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    invoke<MappingEntry[]>("default_mapping")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => {});
  }, []);

  const selectedName = REMOTE_BUTTONS.find((b) => b.key === selected)?.name || selected;
  const actionLabel =
    ACTION_CATEGORIES.find((c) => c.key === category)
      ?.actions.find((a) => a.key === action)?.label || action;

  async function save() {
    if (!isTauri()) return;
    try {
      await invoke("save_mapping", {
        edit: { button: selected, trigger, action },
      });
      setSaveMsg(`已保存：${selectedName} · ${TRIGGER_LABEL[trigger]} → ${actionLabel}`);
    } catch (err) {
      setSaveMsg(`保存失败: ${err}`);
    }
  }

  return (
    <div className="page">
      
      

      <div className="mapping-wizard">
        <section className="card remote-card">
          <div className="card-title">① 选择按键</div>
          <Xiaomi2ProRemote selected={selected} onSelect={setSelected} />
          <p className="hint current-key">{selectedName}</p>
        </section>

        <section className="card wizard-card">
          

          <div className="wizard-group">
            <div className="wizard-label">② 触发方式</div>
            <div className="trigger-options">
              {TRIGGERS.map((t) => (
                <button
                  key={t.key}
                  className={`trigger-btn ${trigger === t.key ? "active" : ""}`}
                  onClick={() => setTrigger(t.key)}
                >
                  <span className="trigger-name">{t.label}</span>
                  <span className="trigger-desc">{t.desc}</span>
                </button>
              ))}
            </div>
          </div>

          <div className="wizard-group">
            <div className="wizard-label">③ 动作分类</div>
            <div className="category-tabs">
              {ACTION_CATEGORIES.map((c) => (
                <button
                  key={c.key}
                  className={`btn small ${category === c.key ? "primary" : ""}`}
                  onClick={() => {
                    setCategory(c.key);
                    setAction(c.actions[0]?.key || "disabled");
                  }}
                >
                  {c.title}
                </button>
              ))}
            </div>
            <div className="action-grid">
              {ACTION_CATEGORIES.find((c) => c.key === category)?.actions.map((a) => (
                <button
                  key={a.key}
                  className={`btn small ${action === a.key ? "primary" : ""}`}
                  onClick={() => setAction(a.key)}
                >
                  {a.label}
                </button>
              ))}
            </div>
          </div>

          <div className="preview-box">
            <span className="preview-label">即将保存</span>
            <span className="preview-value">
              {selectedName} · {TRIGGER_LABEL[trigger]} → {actionLabel}
            </span>
          </div>

          <div className="actions">
            <button className="btn primary" onClick={save} disabled={!isTauri()}>
              保存此键
            </button>
          </div>
          {saveMsg && <p className="hint">{saveMsg}</p>}
        </section>
      </div>

      <section className="card">
        <div className="card-title">当前映射表</div>
        <div className="mapping-list">
          {mapping.map((b) => (
            <div key={b.button} className="mapping-row">
              <span className="mapping-key">{b.name}</span>
              <span className="mapping-action">{b.action}</span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
