import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";
import {
  ACTION_CATEGORIES,
  FALLBACK_MAPPING,
  REMOTE_BUTTONS,
  TRIGGER_LABEL,
  TRIGGERS,
} from "./mapping/constants";
import type { MappingEntry } from "./mapping/types";

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [category, setCategory] = useState("system");
  const [action, setAction] = useState("return");
  const [saveMsg, setSaveMsg] = useState("");

  useEffect(() => {
    if (!isTauri()) {
      setMapping(FALLBACK_MAPPING);
      return;
    }
    invoke<MappingEntry[]>("get_mappings")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => setMapping(FALLBACK_MAPPING));
  }, []);

  // 选中按键或切换触发方式时，右侧自动展示该按键已绑定的动作。
  useEffect(() => {
    const binding = mapping.find(
      (m) => m.button === selected && m.trigger === trigger
    );
    const actionKey = binding?.action_key || "disabled";
    const cat = ACTION_CATEGORIES.find((c) =>
      c.actions.some((a) => a.key === actionKey)
    );
    setCategory(cat?.key || "other");
    setAction(actionKey);
  }, [mapping, selected, trigger]);

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
      const entry: MappingEntry = {
        button: selected,
        name: selectedName,
        trigger,
        action: actionLabel,
        action_key: action,
      };
      setMapping((prev) => {
        const idx = prev.findIndex(
          (m) => m.button === selected && m.trigger === trigger
        );
        if (idx >= 0) {
          const next = [...prev];
          next[idx] = entry;
          return next;
        }
        return [...prev, entry];
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
                  title={t.desc}
                  onClick={() => setTrigger(t.key)}
                >
                  <span className="trigger-name">{t.label}</span>
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
            <div key={`${b.button}-${b.trigger}`} className="mapping-row">
              <span className="mapping-key">
                {b.name} · {TRIGGER_LABEL[b.trigger] || b.trigger}
              </span>
              <span className="mapping-action">{b.action}</span>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
