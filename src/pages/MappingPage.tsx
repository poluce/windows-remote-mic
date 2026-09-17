import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { NewProfileDialog } from "../components/NewProfileDialog";
import { ShortcutDialog } from "../components/ShortcutDialog";
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
  formatComboDisplay,
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
import {
  comboLabel,
  deleteShortcut,
  keysId,
  loadShortcuts,
  saveShortcut,
  shortcutNameFor,
  type NamedShortcut,
} from "./mapping/shortcuts";
import { GLOBAL_SCOPE, type AppProfileView, type MappingEntry, type ScopeId } from "./mapping/types";

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);
  const [profiles, setProfiles] = useState<AppProfileView[]>([]);
  const [shortcuts, setShortcuts] = useState<NamedShortcut[]>([]);
  const [scope, setScope] = useState<ScopeId>(GLOBAL_SCOPE);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [category, setCategory] = useState("system");
  const [action, setAction] = useState("return");
  const [comboTokens, setComboTokens] = useState<string[]>([]);
  const remoteArtRef = useRef<HTMLDivElement | null>(null);
  const [saveMsg, setSaveMsg] = useState("");
  const [newOpen, setNewOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<AppProfileView | null>(null);
  /** 快捷键对话框：`editing` 为 null 表示新建。 */
  const [shortcutDialog, setShortcutDialog] = useState<{
    open: boolean;
    editing: NamedShortcut | null;
  }>({ open: false, editing: null });

  /**
   * 遥控器图形按卡片剩下的高度缩放。
   *
   * 它是定尺寸的像素图（105×345，内部全是绝对定位的按键），没法用百分比或
   * flex 压缩，只能整体 zoom。而它默认放大 1.3 倍后有 448px 高——**这是整页
   * 704px 底线的来源**：窗口一矮，卡片压不下去，滚动条就出来了。
   *
   * 所以量出 `.remote-art` 实际分到多高，反推 zoom。这样窗口怎么拉都不会把
   * 遥控器挤出去，也不会让它把整页顶高。
   */
  useEffect(() => {
    const box = remoteArtRef.current;
    if (!box) return;
    const BASE_HEIGHT = 345; // 遥控器未缩放的原始高度，见 Xiaomi2ProRemote.css
    const apply = () => {
      const zoom = Math.min(1.3, Math.max(0.4, box.clientHeight / BASE_HEIGHT));
      box.style.setProperty("--remote-zoom", zoom.toFixed(3));
    };
    // 先同步量一次：只靠 ResizeObserver 的话，第一帧之前遥控器一直是
    // 未缩放的 448px，会先闪一下、也可能正好卡在「量不到」的状态。
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

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
    loadShortcuts().then(setShortcuts);
  }, [refreshProfiles]);

  // 组合键在界面上显示名字而不是「快捷键 左Ctrl+K」。库纯粹是显示层，
  // 在这里一次性把标签换掉，矩阵和下面的保存逻辑都不用认识「自定义快捷键」。
  const withNames = useCallback(
    (entries: MappingEntry[]) =>
      entries.map((e) => ({
        ...e,
        action: shortcutNameFor(shortcuts, e.action_key) ?? e.action,
      })),
    [shortcuts],
  );

  const activeProfile = profiles.find((p) => p.id === scope);

  /*
   * 两套 Map，刻意分开：
   *
   * - **逻辑用**（下面这对，不带名字）：判断「这一格真正生效的是哪条绑定」。
   * - **显示用**（View 那对）：把组合键换成自定义名字。
   *
   * 混用会造成一个很隐蔽的 bug：withNames 依赖 shortcuts，库一变这两个 Map
   * 就是新对象，下面那个同步 effect 会跟着重跑，按当前格子的绑定把分类重置
   * 回「系统」——于是在自定义分类里新建完一条快捷键，那一栏立刻被切走，
   * 刚建的东西你看不到也选不上。名字是显示层，不该影响「哪条绑定生效」。
   */
  const globalCells = useMemo(() => toCellMap(mapping), [mapping]);
  const overrideCells = useMemo(
    () => toCellMap(activeProfile?.bindings ?? []),
    [activeProfile],
  );
  const globalView = useMemo(() => toCellMap(withNames(mapping)), [mapping, withNames]);
  const overrideView = useMemo(
    () => toCellMap(withNames(activeProfile?.bindings ?? [])),
    [activeProfile, withNames],
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
      return;
    }
    const cat = ACTION_CATEGORIES.find((c) =>
      c.actions.some((a) => a.key === actionKey)
    );
    setCategory(cat?.key || "other");
    setAction(actionKey);
    setComboTokens([]);
  }, [globalCells, overrideCells, selected, trigger]);

  // 触发方式随按键切换：麦克风只有按下/松开，其它键只有单击/双击/长按。
  useEffect(() => {
    const available = triggersFor(selected);
    if (!available.some((t) => t.key === trigger)) {
      setTrigger(available[0]?.key || "single_click");
    }
  }, [selected, trigger]);

  const availableTriggers = triggersFor(selected);

  const selectedName = REMOTE_BUTTONS.find((b) => b.key === selected)?.name || selected;
  const actionLabel =
    category === COMBO_CATEGORY
      ? comboLabel(shortcuts, comboTokens)
      : ACTION_CATEGORIES.find((c) => c.key === category)
          ?.actions.find((a) => a.key === action)?.label || action;

  const selectedCell = cellKey(selected, trigger);
  // 这两条只用于显示（覆盖说明里的「全局是〈什么〉」），所以取带名字的那套
  const selectedOverride = overrideView.get(selectedCell);
  const selectedGlobal = globalView.get(selectedCell);
  const isGlobalScope = scope === GLOBAL_SCOPE;

  const scopeItems: ScopeItem[] = [
    { id: GLOBAL_SCOPE, label: "全局", removable: false },
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
      if (!comboTokens.length) {
        setSaveMsg("请先选一个自定义快捷键，或用「+ 新建快捷键」建一个");
        return;
      }
      actionToSave = toComboActionKey(comboTokens);
      savedLabel = comboLabel(shortcuts, comboTokens);
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

  /**
   * 存一条自定义快捷键。
   *
   * 编辑已有条目时**改了按键就等于换了一条**（keys 是身份）：先把旧的那条删掉
   * 再存新的，否则会留下一条没人用的孤儿。只改名字则 keys 不变，后端按 keys
   * 就地更新。
   */
  async function handleSaveShortcut(name: string, keys: string[]) {
    const editing = shortcutDialog.editing;
    setShortcutDialog({ open: false, editing: null });
    try {
      if (editing && keysId(editing.keys) !== keysId(keys)) {
        await deleteShortcut(editing.keys);
      }
      setShortcuts(await saveShortcut(name, keys));
      // 存完就把这一条选上——用户建它就是为了用它。
      setCategory(COMBO_CATEGORY);
      setAction(CUSTOM_COMBO_ACTION);
      setComboTokens(keys);
      setSaveMsg(`已保存快捷键「${name}」，再点「保存」绑给这个键`);
    } catch (err) {
      setSaveMsg(`保存快捷键失败: ${err}`);
    }
  }

  async function handleDeleteShortcut(keys: string[]) {
    setShortcutDialog({ open: false, editing: null });
    try {
      setShortcuts(await deleteShortcut(keys));
      setSaveMsg("已删掉这条命名。已经绑上它的按键不受影响，只是不再显示名字");
    } catch (err) {
      setSaveMsg(`删除快捷键失败: ${err}`);
    }
  }

  return (
    <div className="page page-fill">
      <div className="section-label">按键配置</div>

      <section className="card">
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
          <div className="remote-art" ref={remoteArtRef}>
            <Xiaomi2ProRemote selected={selected} onSelect={setSelected} />
          </div>
          <p className="hint current-key">{selectedName}</p>
        </section>

        {/* 向导与映射表合并成一张卡，左右水平排布 */}
        <section className="card wizard-card merged-card">
          <div className="merged-cols">
            <div className="merged-col">
              {/* 上半：按钮墙，窄了就在这里自己滚 */}
              <div className="wizard-scroll">
              <div className="wizard-group">
                <div className="wizard-label">触发方式</div>
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
                <div className="wizard-label">动作分类</div>
                <div className="category-tabs">
                  {ACTION_CATEGORIES.map((c) => (
                    <button
                      key={c.key}
                      className={`btn small ${category === c.key ? "primary" : ""}`}
                      onClick={() => {
                        setCategory(c.key);
                        setAction(c.actions[0]?.key || "disabled");
                        if (c.key !== COMBO_CATEGORY) setComboTokens([]);
                      }}
                    >
                      {c.title}
                    </button>
                  ))}
                </div>
                {category === COMBO_CATEGORY ? (
                  <div className="shortcut-grid">
                    {shortcuts.map((sc) => (
                      <span className="shortcut-item" key={keysId(sc.keys)}>
                        <button
                          type="button"
                          className={`shortcut-btn${keysId(sc.keys) === keysId(comboTokens) ? " active" : ""}`}
                          onClick={() => setComboTokens(sc.keys)}
                        >
                          <span className="shortcut-name">{sc.name}</span>
                          <span className="shortcut-keys">{formatComboDisplay(sc.keys)}</span>
                        </button>
                        <button
                          type="button"
                          className="shortcut-edit"
                          title="改名 / 删除"
                          aria-label={`编辑「${sc.name}」`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setShortcutDialog({ open: true, editing: sc });
                          }}
                        >
                          ✎
                        </button>
                      </span>
                    ))}
                    <button
                      type="button"
                      className="shortcut-btn add"
                      onClick={() => setShortcutDialog({ open: true, editing: null })}
                    >
                      <span className="shortcut-name">+ 新建快捷键</span>
                    </button>
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
              </div>

              {/* 下半：覆盖说明与保存钉在这一栏底部，按钮墙再长也够得着 */}
              <div className="wizard-foot">
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
                global={globalView}
                override={overrideView}
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

      <ShortcutDialog
        open={shortcutDialog.open}
        editing={shortcutDialog.editing}
        onCancel={() => setShortcutDialog({ open: false, editing: null })}
        onSave={handleSaveShortcut}
        onDelete={handleDeleteShortcut}
      />
    </div>
  );
}
