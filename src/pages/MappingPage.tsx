import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";
import {
  ACTION_CATEGORIES,
  FALLBACK_MAPPING,
  REMOTE_BUTTONS,
  TRIGGER_LABEL,
  triggersFor,
} from "./mapping/constants";
import {
  COMBO_CATEGORY,
  CUSTOM_COMBO_ACTION,
  canonicalizeCombo,
  formatComboDisplay,
  keyEventToCombo,
  modifierTokenFromCode,
  parseComboActionKey,
  toComboActionKey,
} from "./mapping/combo";
import type { MappingEntry } from "./mapping/types";

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [category, setCategory] = useState("system");
  const [action, setAction] = useState("return");
  const [comboTokens, setComboTokens] = useState<string[]>([]);
  const [comboDraft, setComboDraft] = useState("");
  const [capturing, setCapturing] = useState(false);
  const pendingComboRef = useRef<string[]>([]);
  const heldModsRef = useRef<string[]>([]);
  const [saveMsg, setSaveMsg] = useState("");
  const [longPressMs, setLongPressMs] = useState(550);
  const [doubleClickMs, setDoubleClickMs] = useState(300);
  const [eatEnabled, setEatEnabled] = useState<boolean | null>(null);
  const [eatBusy, setEatBusy] = useState(false);

  useEffect(() => {
    if (!isTauri()) {
      setEatEnabled(true);
      return;
    }
    invoke<boolean>("get_hid_tap_eat")
      .then(setEatEnabled)
      .catch(() => setEatEnabled(true));
  }, []);

  async function toggleEat() {
    if (!isTauri() || eatEnabled === null || eatBusy) {
      return;
    }
    setEatBusy(true);
    try {
      const next = await invoke<boolean>("set_hid_tap_eat", {
        enabled: !eatEnabled,
      });
      setEatEnabled(next);
      setSaveMsg(
        next
          ? "已开启拦截：系统不再响应遥控器按键，只由本应用注入映射动作"
          : "已关闭拦截：系统会同时响应遥控器按键",
      );
    } catch (err) {
      setSaveMsg(`切换失败：${err}`);
    } finally {
      setEatBusy(false);
    }
  }

  useEffect(() => {
    if (!isTauri()) {
      setMapping(FALLBACK_MAPPING);
      return;
    }
    invoke<MappingEntry[]>("get_mappings")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => setMapping(FALLBACK_MAPPING));
    invoke<{ long_press_ms: number; double_click_ms: number }>("get_trigger_timing")
      .then((t) => {
        setLongPressMs(t.long_press_ms);
        setDoubleClickMs(t.double_click_ms);
      })
      .catch(() => {});
  }, []);

  async function saveTiming(nextLong: number, nextDouble: number) {
    if (!isTauri()) return;
    setLongPressMs(nextLong);
    setDoubleClickMs(nextDouble);
    try {
      await invoke("set_trigger_timing", {
        longPressMs: nextLong,
        doubleClickMs: nextDouble,
      });
    } catch (err) {
      setSaveMsg(`保存触发时间失败: ${err}`);
    }
  }

  // 选中按键或切换触发方式时，右侧自动展示该按键已绑定的动作。
  useEffect(() => {
    const binding = mapping.find(
      (m) => m.button === selected && m.trigger === trigger
    );
    const actionKey = binding?.action_key || "disabled";
    const combo = parseComboActionKey(actionKey);
    if (combo) {
      setCategory(COMBO_CATEGORY);
      setAction(CUSTOM_COMBO_ACTION);
      setComboTokens(combo);
      setComboDraft(combo.join("+"));
      pendingComboRef.current = [];
      heldModsRef.current = [];
      setCapturing(false);
      return;
    }
    const cat = ACTION_CATEGORIES.find((c) =>
      c.actions.some((a) => a.key === actionKey)
    );
    setCategory(cat?.key || "other");
    setAction(actionKey);
    setComboTokens([]);
    setComboDraft("");
    pendingComboRef.current = [];
    heldModsRef.current = [];
    setCapturing(false);
  }, [mapping, selected, trigger]);

  function commitCombo(tokens: string[]) {
    pendingComboRef.current = [];
    heldModsRef.current = [];
    setComboTokens(tokens);
    setComboDraft(tokens.join("+"));
    setSaveMsg("");
  }

  function onComboFocus() {
    pendingComboRef.current = [];
    heldModsRef.current = [];
    setCapturing(true);
    setSaveMsg("");
  }

  function onComboBlur() {
    if (pendingComboRef.current.length) {
      const tokens = canonicalizeCombo(pendingComboRef.current);
      if (tokens?.length) {
        setComboTokens(tokens);
        setComboDraft(tokens.join("+"));
      }
    }
    pendingComboRef.current = [];
    heldModsRef.current = [];
    setCapturing(false);
  }

  function onComboKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.currentTarget.blur();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const captured = keyEventToCombo(e.nativeEvent, heldModsRef.current);
    if (!captured) {
      setSaveMsg("该按键暂不支持，请换字母、数字、F1–F12，或左/右 Ctrl、Shift、Alt、Win");
      return;
    }
    const mod = modifierTokenFromCode(e.code);
    if (mod && !heldModsRef.current.includes(mod)) {
      heldModsRef.current = [...heldModsRef.current, mod];
    }
    pendingComboRef.current = captured.tokens;
    setComboDraft(captured.tokens.join("+"));
    if (captured.complete) commitCombo(captured.tokens);
  }

  function onComboKeyUp(e: KeyboardEvent<HTMLInputElement>) {
    e.preventDefault();
    e.stopPropagation();
    const mod = modifierTokenFromCode(e.code);
    if (!mod) return;
    const remaining = heldModsRef.current.filter((m) => m !== mod);
    if (remaining.length === 0 && pendingComboRef.current.length) {
      const tokens = canonicalizeCombo(pendingComboRef.current);
      if (tokens?.length) commitCombo(tokens);
      return;
    }
    heldModsRef.current = remaining;
  }

  // 触发方式随按键切换：麦克风只有按下/松开，其它键只有单击/双击/长按。
  useEffect(() => {
    const available = triggersFor(selected);
    if (!available.some((t) => t.key === trigger)) {
      setTrigger(available[0]?.key || "single_click");
    }
  }, [selected, trigger]);

  const availableTriggers = triggersFor(selected);

  const selectedName = REMOTE_BUTTONS.find((b) => b.key === selected)?.name || selected;
  const comboLabel = comboTokens.length
    ? `快捷键 ${formatComboDisplay(comboTokens)}`
    : "自定义快捷键";
  const actionLabel =
    category === COMBO_CATEGORY
      ? comboLabel
      : ACTION_CATEGORIES.find((c) => c.key === category)
          ?.actions.find((a) => a.key === action)?.label || action;

  async function save() {
    if (!isTauri()) return;
    let actionToSave = action;
    let savedLabel = actionLabel;
    if (category === COMBO_CATEGORY) {
      let tokens = comboTokens;
      if (!tokens.length && comboDraft.trim()) {
        const parsed = canonicalizeCombo(
          comboDraft
            .split("+")
            .map((s) => s.trim().toLowerCase())
            .filter(Boolean),
        );
        if (!parsed) {
          setSaveMsg("快捷键格式无效，例如 rctrl、c 或 lctrl+c");
          return;
        }
        tokens = parsed;
        setComboTokens(parsed);
        setComboDraft(parsed.join("+"));
      }
      if (!tokens.length) {
        setSaveMsg("请先录制或输入快捷键");
        return;
      }
      actionToSave = toComboActionKey(tokens);
      savedLabel = `快捷键 ${formatComboDisplay(tokens)}`;
    }
    try {
      await invoke("save_mapping", {
        edit: { button: selected, trigger, action: actionToSave },
      });
      const entry: MappingEntry = {
        button: selected,
        name: selectedName,
        trigger,
        action: savedLabel,
        action_key: actionToSave,
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
      setSaveMsg(`已保存：${selectedName} · ${TRIGGER_LABEL[trigger]} → ${savedLabel}`);
    } catch (err) {
      setSaveMsg(`保存失败: ${err}`);
    }
  }

  return (
    <div className="page">
      <div className="section-label">按键配置</div>

      <section className="card eat-card">
        <div className="eat-info">
          <div className="eat-title">拦截 HID 按键信号</div>
          <p className="hint">
            {eatEnabled === null
              ? "读取中…"
              : eatEnabled
                ? "已开启：系统不响应遥控器按键，只由本应用注入映射动作"
                : "已关闭：系统会同时响应遥控器按键"}
          </p>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={eatEnabled === true}
          className={`switch${eatEnabled ? " on" : ""}`}
          onClick={toggleEat}
          disabled={eatEnabled === null || eatBusy || !isTauri()}
        >
          <span className="switch-thumb" />
        </button>
      </section>

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
              {availableTriggers.map((t) => (
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
            <div className="timing-row">
              <label className="timing-label">
                长按判定
                <select
                  value={longPressMs}
                  onChange={(e) => saveTiming(Number(e.target.value), doubleClickMs)}
                >
                  <option value={400}>0.4 秒</option>
                  <option value={550}>0.55 秒</option>
                  <option value={700}>0.7 秒</option>
                  <option value={1000}>1.0 秒</option>
                </select>
              </label>
              <label className="timing-label">
                双击间隔
                <select
                  value={doubleClickMs}
                  onChange={(e) => saveTiming(longPressMs, Number(e.target.value))}
                >
                  <option value={200}>0.2 秒</option>
                  <option value={300}>0.3 秒</option>
                  <option value={400}>0.4 秒</option>
                  <option value={500}>0.5 秒</option>
                </select>
              </label>
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
                    setCapturing(false);
                    if (c.key !== COMBO_CATEGORY) {
                      setComboTokens([]);
                      setComboDraft("");
                    }
                  }}
                >
                  {c.title}
                </button>
              ))}
            </div>
            {category === COMBO_CATEGORY ? (
              <div className="combo-capture">
                <div className="combo-capture-row">
                  <input
                    className={`combo-input${capturing ? " listening" : ""}`}
                    readOnly
                    value={
                      capturing && comboDraft
                        ? formatComboDisplay(comboDraft.split("+"))
                        : comboTokens.length
                          ? formatComboDisplay(comboTokens)
                          : ""
                    }
                    placeholder={capturing ? "按下快捷键" : "点击此处，然后按下快捷键"}
                    onFocus={onComboFocus}
                    onBlur={onComboBlur}
                    onKeyDown={onComboKeyDown}
                    onKeyUp={onComboKeyUp}
                  />
                  {comboTokens.length > 0 && (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => {
                        pendingComboRef.current = [];
                        heldModsRef.current = [];
                        setComboTokens([]);
                        setComboDraft("");
                        setCapturing(false);
                      }}
                    >
                      清除
                    </button>
                  )}
                </div>
              </div>
            ) : (
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
            )}
          </div>

          <div className="preview-box">
            <span className="preview-label">即将保存</span>
            <span className="preview-value">
              {selectedName} · {TRIGGER_LABEL[trigger]} → {actionLabel}
            </span>
          </div>

          <div className="actions">
            <button
              className="btn primary"
              onClick={save}
              disabled={!isTauri()}
            >
              保存此键
            </button>
          </div>
          {saveMsg && <p className="hint">{saveMsg}</p>}
        </section>
      </div>

      <div className="section-label">映射表</div>
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
