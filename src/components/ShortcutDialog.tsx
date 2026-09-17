import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  canonicalizeCombo,
  formatComboDisplay,
  keyEventToCombo,
  modifierTokenFromCode,
} from "../pages/mapping/combo";
import type { NamedShortcut } from "../pages/mapping/shortcuts";

/**
 * 新建 / 编辑一条自定义快捷键。
 *
 * 一个对话框同时承担两件事：改名和重新录制。编辑已有条目时右下角多一个
 * 「删除」——删除是这里唯一不可逆的操作，放进对话框里多一次确认的机会，
 * 好过在按钮角上放个一点就没的 ×。
 */
export function ShortcutDialog({
  open,
  editing,
  onCancel,
  onSave,
  onDelete,
}: {
  open: boolean;
  /** null = 新建。 */
  editing: NamedShortcut | null;
  onCancel: () => void;
  onSave: (name: string, keys: string[]) => void;
  onDelete: (keys: string[]) => void;
}) {
  const [name, setName] = useState("");
  const [tokens, setTokens] = useState<string[]>([]);
  const [error, setError] = useState("");
  const heldModsRef = useRef<string[]>([]);

  // 每次打开都按「当前编辑的是哪一条」重置，避免带上一次的残留。
  useEffect(() => {
    if (!open) return;
    setName(editing?.name ?? "");
    setTokens(editing?.keys ?? []);
    setError("");
    heldModsRef.current = [];
  }, [open, editing]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: globalThis.KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  function onComboKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.currentTarget.blur();
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    const captured = keyEventToCombo(e.nativeEvent, heldModsRef.current);
    if (!captured) {
      setError("该按键暂不支持，请换字母、数字、F1–F12，或左/右 Ctrl、Shift、Alt、Win");
      return;
    }
    const mod = modifierTokenFromCode(e.code);
    if (mod && !heldModsRef.current.includes(mod)) {
      heldModsRef.current = [...heldModsRef.current, mod];
    }
    setTokens(captured.tokens);
    setError("");
  }

  function onComboKeyUp(e: KeyboardEvent<HTMLInputElement>) {
    e.preventDefault();
    e.stopPropagation();
    const mod = modifierTokenFromCode(e.code);
    if (!mod) return;
    const remaining = heldModsRef.current.filter((m) => m !== mod);
    heldModsRef.current = remaining;
    // 只按住了修饰键、还没按主键：松开时把「已按住的部分」留下当草稿。
    if (remaining.length === 0 && tokens.length && !tokens.some((t) => t.length === 1)) {
      setTokens([]);
    }
  }

  function submit() {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("请给这个快捷键起个名字");
      return;
    }
    if (!tokens.length) {
      setError("请先录制快捷键");
      return;
    }
    const canonical = canonicalizeCombo(tokens);
    if (!canonical?.length) {
      setError("快捷键格式无效");
      return;
    }
    onSave(trimmed, canonical);
  }

  return (
    <div className="dialog-mask open" onClick={onCancel}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>{editing ? "编辑快捷键" : "新建快捷键"}</h3>
        <p className="hint dialog-desc">
          给它起个名字，绑定时列表里就显示这个名字，不用记按键组合。
        </p>

        <div className="field">
          <label htmlFor="sc-name">名称</label>
          <input
            id="sc-name"
            value={name}
            placeholder="例如：发送消息"
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div className="field">
          <label htmlFor="sc-keys">快捷键</label>
          <input
            id="sc-keys"
            className={`combo-input${tokens.length ? "" : " listening"}`}
            readOnly
            value={tokens.length ? formatComboDisplay(tokens) : ""}
            placeholder="点击此处，然后按下快捷键"
            onKeyDown={onComboKeyDown}
            onKeyUp={onComboKeyUp}
          />
          <div className="tip">
            左右修饰键是分开的（左Ctrl 和右Ctrl 不是同一个键）。
            录制时先按住修饰键再按主键。
          </div>
        </div>

        {error && <p className="hint dialog-error">{error}</p>}

        <div className="dialog-actions">
          {editing && (
            <button
              type="button"
              className="btn danger"
              onClick={() => onDelete(editing.keys)}
            >
              删除
            </button>
          )}
          <span className="dialog-spacer" />
          <button type="button" className="btn" onClick={onCancel}>
            取消
          </button>
          <button type="button" className="btn primary" onClick={submit}>
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
