import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NewProfileDialog } from "../components/NewProfileDialog";
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
import { cellKey, toCellMap } from "./mapping/cells";
import { MappingMatrix } from "./mapping/MappingMatrix";
import { ScopeBar, type ScopeItem } from "./mapping/ScopeBar";
import {
  clearProfileBinding,
  createProfile as createProfileRequest,
  deleteProfile,
  loadProfiles,
  saveProfileBinding,
} from "./mapping/profile";
import { GLOBAL_SCOPE, type AppProfileView, type MappingEntry, type ScopeId } from "./mapping/types";

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [profiles, setProfiles] = useState<AppProfileView[]>([]);
  const [scope, setScope] = useState<ScopeId>(GLOBAL_SCOPE);
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
  const [newOpen, setNewOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<AppProfileView | null>(null);

  const refreshProfiles = useCallback(async () => {
    setProfiles(await loadProfiles());
  }, []);

  useEffect(() => {
    if (!isTauri()) {
      setMapping(FALLBACK_MAPPING);
      return;
    }
    invoke<MappingEntry[]>("get_mappings")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => setMapping(FALLBACK_MAPPING));
    refreshProfiles();
  }, [refreshProfiles]);

  const globalCells = useMemo(() => toCellMap(mapping), [mapping]);
  const activeProfile = profiles.find((p) => p.id === scope);
  const overrideCells = useMemo(
    () => toCellMap(activeProfile?.bindings ?? []),
    [activeProfile],
  );

  // 选中按键或切换触发方式时，向导自动载入**当前作用范围下真正生效**的那条绑定：
  // 先问这份应用配置有没有覆盖，没有才落到全局。和调度器的解析顺序一致，
  // 否则界面上显示的动作和实际按下去发生的会对不上。
  useEffect(() => {
    const binding =
      overrideCells.get(cellKey(selected, trigger)) ??
      globalCells.get(cellKey(selected, trigger));
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
  }, [globalCells, overrideCells, selected, trigger]);

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

  const selectedCell = cellKey(selected, trigger);
  const selectedOverride = overrideCells.get(selectedCell);
  const selectedGlobal = globalCells.get(selectedCell);
  const isGlobalScope = scope === GLOBAL_SCOPE;

  const scopeItems: ScopeItem[] = [
    { id: GLOBAL_SCOPE, label: "全局（所有应用）", removable: false },
    ...profiles.map((p) => ({
      id: p.id,
      label: p.name,
      color: p.icon_color ?? undefined,
      removable: true,
    })),
  ];

  async function save() {
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
    if (!isTauri()) return;

    const where = `${selectedName} · ${TRIGGER_LABEL[trigger]}`;
    try {
      if (isGlobalScope) {
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
        setSaveMsg(`已保存：${where} → ${savedLabel}`);
      } else {
        await saveProfileBinding(scope, selected, trigger, actionToSave);
        // 后端写完文件并重载了调度器，这里把它的结果读回来当唯一事实来源，
        // 不在前端自己拼一份「应该是这样」的副本。
        await refreshProfiles();
        setSaveMsg(`已保存到「${activeProfile?.name}」：${where} → ${savedLabel}`);
      }
    } catch (err) {
      setSaveMsg(`保存失败: ${err}`);
    }
  }

  /** 「改用全局」：只撤掉选中的这一格，这份配置的其它格子不动。 */
  async function useGlobal() {
    if (isGlobalScope) return;
    const where = `${selectedName} · ${TRIGGER_LABEL[trigger]}`;
    try {
      await clearProfileBinding(scope, selected, trigger);
      await refreshProfiles();
      setSaveMsg(`「${where}」已改回沿用全局`);
    } catch (err) {
      setSaveMsg(`取消失败: ${err}`);
    }
  }

  async function handleCreate(name: string, process: string, title: string) {
    setNewOpen(false);
    try {
      const id = await createProfileRequest(name, process, title);
      await refreshProfiles();
      setScope(id);
      setSaveMsg("已新建这份配置，改过的按键会自动覆盖全局");
    } catch (err) {
      setSaveMsg(`新建失败: ${err}`);
    }
  }

  async function handleDelete() {
    const target = pendingDelete;
    setPendingDelete(null);
    if (!target) return;
    try {
      await deleteProfile(target.id);
      await refreshProfiles();
      // 删掉的正好是当前正在看的那一份，就退回全局。
      if (scope === target.id) setScope(GLOBAL_SCOPE);
      setSaveMsg(`已删除「${target.name}」的配置，它的按键回到全局`);
    } catch (err) {
      setSaveMsg(`删除失败: ${err}`);
    }
  }

  return (
    <div className="page">
      <div className="section-label">按键配置</div>

      <section className="card">
        <div className="card-title">作用范围</div>
        <ScopeBar
          items={scopeItems}
          active={scope}
          onSelect={setScope}
          onRemove={(item) =>
            setPendingDelete(profiles.find((p) => p.id === item.id) ?? null)
          }
          onNew={() => setNewOpen(true)}
        />
      </section>

      <div className="mapping-wizard">
        <section className="card remote-card">
          <div className="card-title">① 选择按键</div>
          <Xiaomi2ProRemote selected={selected} onSelect={setSelected} />
          <p className="hint current-key">{selectedName}</p>
        </section>

        {/* 向导与映射表合并成一张卡，左右水平排布 */}
        <section className="card wizard-card merged-card">
          <div className="merged-cols">
            <div className="merged-col">
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

              {selectedOverride && (
                <p className="hint override-note">
                  这一格已改成「{selectedOverride.action}」，全局是「
                  {selectedGlobal?.action ?? "未绑定"}」。
                </p>
              )}

              <div className="actions">
                <button className="btn primary" onClick={save} disabled={!isTauri()}>
                  {isGlobalScope ? "保存此键" : `保存到「${activeProfile?.name}」`}
                </button>
                {!isGlobalScope && selectedOverride && (
                  <button className="btn" onClick={useGlobal}>
                    改用全局
                  </button>
                )}
              </div>
              {saveMsg && <p className="hint">{saveMsg}</p>}
            </div>

            <div className="merged-col">
              <MappingMatrix
                title={isGlobalScope ? "映射表" : `映射表 · ${activeProfile?.name ?? ""}`}
                hint={
                  isGlobalScope
                    ? undefined
                    : `蓝底 ＝ 这份配置改过的，其余沿用全局`
                }
                buttons={REMOTE_BUTTONS}
                triggers={triggersFor("ok")}
                global={globalCells}
                override={overrideCells}
                selected={{ button: selected, trigger }}
                onSelect={(button, trig) => {
                  setSelected(button);
                  setTrigger(trig);
                }}
              />
            </div>
          </div>
        </section>
      </div>

      <NewProfileDialog
        open={newOpen}
        onCancel={() => setNewOpen(false)}
        onCreate={handleCreate}
      />

      <ConfirmDialog
        open={pendingDelete !== null}
        danger
        title="删除这份配置？"
        desc={
          pendingDelete
            ? `「${pendingDelete.name}」将不再有专属配置，所有按键沿用全局。`
            : undefined
        }
        confirmLabel="删除"
        onConfirm={handleDelete}
        onCancel={() => setPendingDelete(null)}
      />
    </div>
  );
}
