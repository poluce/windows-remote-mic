import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  canonicalizeCombo,
  comboFromHeld,
  formatComboDisplay,
  hasMainKey,
  tokenFromEvent,
} from "../pages/mapping/combo";
import type { NamedShortcut } from "../pages/mapping/shortcuts";

const UNSUPPORTED_KEY =
  "该按键暂不支持，请换字母、数字、F1–F12，或左/右 Ctrl、Shift、Alt、Win";

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
  /** 此刻物理上按着的全部按键（修饰键 + 主键），按按下顺序。 */
  const heldRef = useRef<string[]>([]);
  /** 最近一次「录全了」的组合。松手时回到它，所以录制结果不会被松手抹掉。 */
  const completeRef = useRef<string[]>([]);

  // 每次打开都按「当前编辑的是哪一条」重置，避免带上一次的残留。
  useEffect(() => {
    if (!open) return;
    const initial = editing?.keys ?? [];
    setName(editing?.name ?? "");
    setTokens(initial);
    setError("");
    heldRef.current = [];
    completeRef.current = initial;
  }, [open, editing]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: globalThis.KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    // 窗口切走（Alt+Tab、点别的窗口）时剩下的 keyup 就收不到了，
    // 不清账本会让那个修饰键一直粘着，污染下一次录制。
    function dropHeld() {
      heldRef.current = [];
    }
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", dropHeld);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", dropHeld);
    };
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
    const token = tokenFromEvent(e.nativeEvent);
    if (!token) {
      setError(UNSUPPORTED_KEY);
      return;
    }
    // 已经按着了就不再记账：按住不放会一直发 keydown，但组合里只该出现一次。
    if (!heldRef.current.includes(token)) {
      heldRef.current = [...heldRef.current, token];
    }
    const captured = comboFromHeld(heldRef.current);
    if (!captured) {
      setError(UNSUPPORTED_KEY);
      return;
    }
    if (captured.complete) completeRef.current = captured.tokens;
    setTokens(captured.tokens);
    setError("");
  }

  function onComboKeyUp(e: KeyboardEvent<HTMLInputElement>) {
    e.preventDefault();
    e.stopPropagation();
    const token = tokenFromEvent(e.nativeEvent);
    if (!token) return;
    heldRef.current = heldRef.current.filter((t) => t !== token);
    // 还按着别的键就保持现状，让用户看到自己正按的组合。
    if (heldRef.current.length) return;
    // 全部松开了：录全了就留着，只按过修饰键就退回上一次的有效值。
    setTokens(completeRef.current);
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
    if (!hasMainKey(tokens)) {
      setError("再按一个主键（字母、数字、F1–F12 或 Enter/Tab 等）");
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
            左右修饰键是分开的：左Ctrl 和右Ctrl 不是同一个键；修饰键和主键先按哪个都行。
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
