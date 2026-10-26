import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { FALLBACK_CATALOG } from "./constants";
import { cellKey, toCellMap } from "./cells";
import type { ScopeItem } from "./ScopeBar";
import {
  COMBO_CATEGORY,
  CUSTOM_COMBO_ACTION,
  parseComboActionKey,
  toComboActionKey,
} from "./combo";
import {
  clearProfileBinding,
  createProfile as createProfileRequest,
  deleteProfile,
  loadProfiles,
  saveProfileBinding,
} from "./profile";
import {
  comboLabel,
  deleteShortcut,
  keysId,
  loadShortcuts,
  saveShortcut,
  shortcutNameFor,
  type NamedShortcut,
} from "./shortcuts";
import {
  GLOBAL_SCOPE,
  type AppProfileView,
  type MappingCatalog,
  type MappingEntry,
  type ScopeId,
  type TriggerOption,
} from "./types";

export function useMappingEditor() {
  // 按键/触发/动作类别/默认绑定都由后端 `get_mapping_catalog` 单一来源提供；
  // 本地 FALLBACK_CATALOG 只用于浏览器预览与后端异常降级。
  const [catalog, setCatalog] = useState<MappingCatalog>(FALLBACK_CATALOG);
  const [mapping, setMapping] = useState<MappingEntry[]>(
    FALLBACK_CATALOG.default_bindings,
  );
  const [profiles, setProfiles] = useState<AppProfileView[]>([]);
  const [shortcuts, setShortcuts] = useState<NamedShortcut[]>([]);
  const [scope, setScope] = useState<ScopeId>(GLOBAL_SCOPE);
  const [selected, setSelected] = useState("ok");
  const [trigger, setTrigger] = useState("single_click");
  const [category, setCategory] = useState("system");
  const [action, setAction] = useState("return");
  const [comboTokens, setComboTokens] = useState<string[]>([]);
  const [saveMsg, setSaveMsg] = useState("");
  const [newOpen, setNewOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<AppProfileView | null>(null);
  /** 快捷键对话框：`editing` 为 null 表示新建。 */
  const [shortcutDialog, setShortcutDialog] = useState<{
    open: boolean;
    editing: NamedShortcut | null;
  }>({ open: false, editing: null });
  /** 映射表操作反馈（删除后带撤销入口）。 */
  const [toast, setToast] = useState<{
    message: string;
    undo?: () => Promise<void>;
  } | null>(null);
  const toastTimer = useRef<number | null>(null);

  const dismissToast = useCallback(() => {
    if (toastTimer.current !== null) {
      window.clearTimeout(toastTimer.current);
      toastTimer.current = null;
    }
    setToast(null);
  }, []);

  const showToast = useCallback(
    (message: string, undo?: () => Promise<void>) => {
      if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
      setToast({ message, undo });
      toastTimer.current = window.setTimeout(() => {
        setToast(null);
        toastTimer.current = null;
      }, 6000);
    },
    [],
  );

  useEffect(
    () => () => {
      if (toastTimer.current !== null) window.clearTimeout(toastTimer.current);
    },
    [],
  );

  const refreshProfiles = useCallback(async () => {
    setProfiles(await loadProfiles());
  }, []);

  // 全局映射的唯一事实来源在后端（config.json → 调度器），保存后统一从这里重读，
  // 前端不再自己拼一份「应该是这样」的副本。
  const refreshMappings = useCallback(async () => {
    // 空列表 = 用户真的把所有绑定都删光了，不能再回退默认映射，
    // 否则界面显示默认、调度器却是空的。
    const list = await invoke<MappingEntry[]>("get_mappings");
    setMapping(list);
  }, []);

  // 智能体（MCP）改完配置后，后端会 emit config-changed；这里把唯一事实来源重读一遍。
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen("config-changed", () => {
      // 智能体刚改过配置：之前删除留下的撤销快照可能已经过期，
      // 先清掉，避免用户点撤销时把智能体的新改动覆盖回去。
      dismissToast();
      void refreshMappings().catch((err) => {
        setSaveMsg(`智能体修改后刷新映射失败: ${err}`);
      });
      void refreshProfiles().catch((err) => {
        setSaveMsg(`智能体修改后刷新应用配置失败: ${err}`);
      });
      loadShortcuts().then(setShortcuts);
    })
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [dismissToast, refreshMappings, refreshProfiles]);

  useEffect(() => {
    if (!isTauri()) {
      setMapping(FALLBACK_CATALOG.default_bindings);
      return;
    }
    void (async () => {
      // 先取目录（唯一事实来源），再取当前绑定；绑定为空时用目录里的默认绑定。
      let remoteCatalog = FALLBACK_CATALOG;
      try {
        remoteCatalog = await invoke<MappingCatalog>("get_mapping_catalog");
        setCatalog(remoteCatalog);
      } catch (err) {
        void invoke("log_message", {
          message: `[mapping] 读取映射目录失败，使用本地兜底：${String(err)}`,
        }).catch(() => {});
      }
      try {
        const list = await invoke<MappingEntry[]>("get_mappings");
        setMapping(list);
      } catch {
        setMapping(remoteCatalog.default_bindings);
      }
    })();
    refreshProfiles();
    loadShortcuts().then(setShortcuts);
  }, [refreshProfiles, refreshMappings]);

  // ---- 目录派生值（唯一事实来源：catalog） ----
  const { buttons, categories, triggers } = catalog;

  /** 触发方式随按键变化：麦克风只有按下/松开，其它键只有单击/双击/长按。 */
  const triggersFor = useCallback(
    (button: string): TriggerOption[] => {
      const allowed = buttons.find((b) => b.key === button)?.triggers;
      return allowed
        ? triggers.filter((t) => allowed.includes(t.key))
        : triggers;
    },
    [buttons, triggers],
  );

  const triggerLabel = useMemo(
    () =>
      Object.fromEntries(triggers.map((t) => [t.key, t.label])) as Record<
        string,
        string
      >,
    [triggers],
  );

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
    const actionKey = binding?.action_key;
    if (!actionKey) {
      // 未绑定：停在第一个分类的第一项，用户选动作后保存即可建立绑定。
      const first = categories[0];
      setCategory(first?.key || "system");
      setAction(first?.actions[0]?.key || "return");
      setComboTokens([]);
      return;
    }
    const combo = parseComboActionKey(actionKey);
    if (combo) {
      setCategory(COMBO_CATEGORY);
      setAction(CUSTOM_COMBO_ACTION);
      setComboTokens(combo);
      return;
    }
    const cat = categories.find((c) =>
      c.actions.some((a) => a.key === actionKey)
    );
    setCategory(cat?.key || categories[0]?.key || "system");
    setAction(actionKey);
    setComboTokens([]);
  }, [globalCells, overrideCells, selected, trigger, categories]);

  // 触发方式随按键切换：麦克风只有按下/松开，其它键只有单击/双击/长按。
  useEffect(() => {
    const available = triggersFor(selected);
    if (!available.some((t) => t.key === trigger)) {
      setTrigger(available[0]?.key || "single_click");
    }
  }, [selected, trigger, triggersFor]);

  const availableTriggers = triggersFor(selected);
  const matrixTriggers = useMemo(() => triggersFor("ok"), [triggersFor]);

  const selectedName = buttons.find((b) => b.key === selected)?.name || selected;
  const actionLabel =
    category === COMBO_CATEGORY
      ? comboLabel(shortcuts, comboTokens)
      : categories.find((c) => c.key === category)?.actions.find(
          (a) => a.key === action,
        )?.label || action;

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

    const where = `${selectedName} · ${triggerLabel[trigger]}`;
    try {
      if (isGlobalScope) {
        await invoke("save_mapping", {
          edit: { button: selected, trigger, action: actionToSave },
        });
        // 后端写文件并热更新调度器后，把结果读回来当唯一事实来源。
        await refreshMappings();
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
    const where = `${selectedName} · ${triggerLabel[trigger]}`;
    try {
      await clearProfileBinding(scope, selected, trigger);
      await refreshProfiles();
      setSaveMsg(`「${where}」已改回沿用全局`);
    } catch (err) {
      setSaveMsg(`取消失败: ${err}`);
    }
  }

  /** 映射表点击已绑定格子：切换禁用 / 启用（禁用保留动作与记录）。 */
  async function toggleBinding(button: string, trig: string) {
    if (!isTauri() || !isGlobalScope) return;
    const entry = globalCells.get(cellKey(button, trig));
    if (!entry) return;
    const name = buttons.find((b) => b.key === button)?.name ?? button;
    const where = `${name} · ${triggerLabel[trig] ?? trig}`;
    try {
      await invoke("set_mapping_binding_enabled", {
        button,
        trigger: trig,
        enabled: !entry.enabled,
      });
      await refreshMappings();
      setSaveMsg(
        entry.enabled ? `已禁用：${where}（再点一下恢复）` : `已启用：${where}`,
      );
    } catch (err) {
      setSaveMsg(`切换失败: ${err}`);
    }
  }

  /** 映射表右上角 ×：删除一条绑定，并给一条可撤销的 toast。 */
  async function clearBinding(button: string, trig: string) {
    if (!isTauri() || !isGlobalScope) return;
    const entry = globalCells.get(cellKey(button, trig));
    if (!entry) return;
    const name = buttons.find((b) => b.key === button)?.name ?? button;
    const where = `${name} · ${triggerLabel[trig] ?? trig}`;
    try {
      await invoke("clear_mapping_binding", { button, trigger: trig });
      await refreshMappings();
      setSaveMsg(`已取消关联：${where}`);
      showToast(`已取消关联：${where}`, async () => {
        try {
          await invoke("save_mapping", {
            edit: {
              button,
              trigger: trig,
              action: entry.action_key,
              enabled: entry.enabled,
            },
          });
          await refreshMappings();
          setSaveMsg(`已撤销：${where}`);
          dismissToast();
        } catch (err) {
          setSaveMsg(`撤销失败: ${err}`);
        }
      });
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

  const onRemoveScope = useCallback(
    (item: ScopeItem) => {
      setPendingDelete(profiles.find((p) => p.id === item.id) ?? null);
    },
    [profiles],
  );

  return {
    catalog,
    shortcuts,
    scope,
    setScope,
    scopeItems,
    isGlobalScope,
    selected,
    setSelected,
    selectedName,
    trigger,
    setTrigger,
    availableTriggers,
    matrixTriggers,
    triggerLabel,
    category,
    setCategory,
    action,
    setAction,
    actionLabel,
    comboTokens,
    setComboTokens,
    saveMsg,
    save,
    useGlobal,
    toggleBinding,
    clearBinding,
    toast,
    dismissToast,
    selectedOverride,
    selectedGlobal,
    activeProfile,
    globalView,
    overrideView,
    newOpen,
    setNewOpen,
    handleCreate,
    pendingDelete,
    setPendingDelete,
    handleDelete,
    shortcutDialog,
    setShortcutDialog,
    handleSaveShortcut,
    handleDeleteShortcut,
    onRemoveScope,
  };
}
