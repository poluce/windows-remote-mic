import { useMemo, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";

import { Xiaomi2ProRemote } from "../components/Xiaomi2ProRemote";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  ProfileDialog,
  type ProfileFormValues,
} from "../components/ProfileDialog";
import { ShortcutDialog } from "../components/ShortcutDialog";
import { MappingMatrix } from "./mapping/MappingMatrix";
import { ScopeBar } from "./mapping/ScopeBar";
import { COMBO_CATEGORY, formatComboDisplay } from "./mapping/combo";
import { keysId } from "./mapping/shortcuts";
import { useRemoteZoom } from "./mapping/useRemoteZoom";
import { useMappingEditor } from "./mapping/useMappingEditor";

import "./MappingPage.css";

export function MappingPage() {
  const remoteArtRef = useRef<HTMLDivElement | null>(null);
  useRemoteZoom(remoteArtRef);

  const {
    catalog,
    shortcuts,
    scope,
    setScope,
    scopeItems,
    isGlobalScope,
    selected,
    setSelected,
    trigger,
    setTrigger,
    availableTriggers,
    matrixTriggers,
    category,
    setCategory,
    action,
    setAction,
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
    openProfileEditor,
    editingProfile,
    setEditingProfile,
    handleUpdateProfile,
  } = useMappingEditor();

  // 编辑弹窗的初始值：只在「编辑的是哪一份」变化时重算，
  // 否则每次渲染都是新对象，表单会被 effect 反复重置。
  const editInitial = useMemo<ProfileFormValues | null>(() => {
    if (!editingProfile) return null;
    return {
      name: editingProfile.name,
      process: editingProfile.process,
      titleContains: editingProfile.title_contains.join("，"),
      iconColor: editingProfile.icon_color_custom
        ? editingProfile.icon_color
        : null,
    };
  }, [editingProfile]);

  const { buttons, categories } = catalog;

  return (
    <div className="page page-fill">
      <div className="page-head page-head-compact">
        <h1>按键映射</h1>
      </div>

      {/* 作用范围条是工具条而不是内容卡：用轻卡（无扩散阴影），
          避免这一页出现两张"浮起"的卡片与遥控器列抢注意力。 */}
      <section className="card flat">
        <ScopeBar
          items={scopeItems}
          active={scope}
          onSelect={setScope}
          onEdit={openProfileEditor}
          onRemove={onRemoveScope}
          onNew={() => setNewOpen(true)}
        />

      </section>

      <div className="mapping-wizard">
        <section className="remote-card">
          <div className="remote-art" ref={remoteArtRef}>
            <Xiaomi2ProRemote selected={selected} onSelect={setSelected} />
          </div>
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
                    {categories.map((c) => (
                      <button
                        key={c.key}
                        className={`btn small${category === c.key ? " on" : ""}`}
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
                      {categories
                        .find((c) => c.key === category)
                        ?.actions.map((a) => (
                          <button
                            key={a.key}
                            className={`btn small${action === a.key ? " on" : ""}`}
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
                buttons={buttons}
                triggers={matrixTriggers}
                global={globalView}
                override={overrideView}
                selected={{ button: selected, trigger }}
                interactive={isGlobalScope}
                onSelect={(button, trig) => {
                  setSelected(button);
                  setTrigger(trig);
                }}
                onCellClick={(button, trig, bound) => {
                  setSelected(button);
                  setTrigger(trig);
                  // 已绑定格：点击切换禁用/启用（同时设为编辑目标）；空格只选中。
                  if (bound) void toggleBinding(button, trig);
                }}
                onClear={(button, trig) => {
                  void clearBinding(button, trig);
                }}
              />
            </div>
          </div>
        </section>
      </div>

      {toast && (
        <div className="mapping-toast" role="status">
          <span className="mapping-toast-msg">{toast.message}</span>
          {toast.undo && (
            <button
              type="button"
              className="mapping-toast-undo"
              onClick={() => {
                const undo = toast.undo;
                if (undo) void undo();
              }}
            >
              撤销
            </button>
          )}
          <button
            type="button"
            className="mapping-toast-close"
            aria-label="关闭提示"
            onClick={dismissToast}
          >
            ×
          </button>
        </div>
      )}

      <ProfileDialog
        open={newOpen}
        mode="create"
        initial={null}
        onCancel={() => setNewOpen(false)}
        onSubmit={handleCreate}
      />

      {/* 编辑和新建共用同一张表单：改名 / 改匹配规则 / 改内环颜色都在弹窗里 */}
      <ProfileDialog
        open={editingProfile !== null}
        mode="edit"
        initial={editInitial}
        onCancel={() => setEditingProfile(null)}
        onSubmit={handleUpdateProfile}
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
