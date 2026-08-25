import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type MappingEntry = {
  button: string;
  name: string;
  action: string;
};

const REMOTE_BUTTONS: { key: string; name: string }[] = [
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

const TRIGGERS: { key: string; label: string }[] = [
  { key: "single_click", label: "单击" },
  { key: "double_click", label: "双击" },
  { key: "long_press", label: "长按" },
];

const ACTIONS: { key: string; label: string }[] = [
  { key: "arrow_up", label: "↑ 上" },
  { key: "arrow_down", label: "↓ 下" },
  { key: "arrow_left", label: "← 左" },
  { key: "arrow_right", label: "→ 右" },
  { key: "return", label: "回车" },
  { key: "delete_backward", label: "退格" },
  { key: "escape", label: "Esc" },
  { key: "show_desktop", label: "显示桌面" },
  { key: "context_menu", label: "右键菜单" },
  { key: "app_switcher", label: "切换应用" },
  { key: "system_volume_up", label: "音量 +" },
  { key: "system_volume_down", label: "音量 −" },
  { key: "system_volume_mute", label: "静音" },
  { key: "play_pause", label: "播放/暂停" },
  { key: "voice", label: "语音输入（Win+H）" },
  { key: "disabled", label: "禁用" },
];

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [action, setAction] = useState("return");
  const [saveMsg, setSaveMsg] = useState("");

  useEffect(() => {
    if (!isTauri()) return;
    invoke<MappingEntry[]>("default_mapping")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => {});
  }, []);

  const selectedName =
    REMOTE_BUTTONS.find((b) => b.key === selected)?.name || selected;

  async function save() {
    if (!isTauri()) return;
    try {
      await invoke("save_mapping", {
        edit: { button: selected, trigger, action },
      });
      setSaveMsg(`已保存：${selectedName} · ${trigger} → ${action}`);
    } catch (err) {
      setSaveMsg(`保存失败: ${err}`);
    }
  }

  return (
    <div className="page">
      <h2>按键映射</h2>
      <p className="page-sub">
        只用遥控器/鼠标点选按键，设置触发方式和动作。
      </p>

      <div className="mapping-wizard">
        <section className="card remote-card">
          <div className="card-title">遥控器</div>
          <div className="remote-diagram">
            {REMOTE_BUTTONS.map((b) => (
              <button
                key={b.key}
                className={`remote-btn ${selected === b.key ? "active" : ""}`}
                onClick={() => setSelected(b.key)}
              >
                {b.name}
              </button>
            ))}
          </div>
        </section>

        <section className="card wizard-card">
          <div className="card-title">
            正在设置：<strong>{selectedName}</strong>
          </div>

          <div className="wizard-group">
            <div className="wizard-label">触发方式</div>
            <div className="wizard-options">
              {TRIGGERS.map((t) => (
                <button
                  key={t.key}
                  className={`btn ${trigger === t.key ? "primary" : ""}`}
                  onClick={() => setTrigger(t.key)}
                >
                  {t.label}
                </button>
              ))}
            </div>
          </div>

          <div className="wizard-group">
            <div className="wizard-label">动作</div>
            <div className="action-grid">
              {ACTIONS.map((a) => (
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

          <div className="actions">
            <button className="btn primary" onClick={save} disabled={!isTauri()}>
              保存此键
            </button>
            <button className="btn" onClick={() => setSaveMsg("")}>
              清除提示
            </button>
          </div>
          {saveMsg && <p className="hint">{saveMsg}</p>}
        </section>
      </div>

      <section className="card">
        <div className="card-title">当前默认映射（仅展示）</div>
        <div className="mapping-list">
          {mapping.map((b) => (
            <div key={b.button} className="mapping-row">
              <span className="mapping-key">{b.name}</span>
              <span className="mapping-action">{b.action}</span>
              <button className="btn small" onClick={() => setSelected(b.button)}>
                选择
              </button>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
