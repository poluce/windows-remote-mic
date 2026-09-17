/** 自定义快捷键：与后端 `combo:lctrl+c` action_key 对齐。左右修饰键分开记录。 */

export const COMBO_CATEGORY = "combo";
export const CUSTOM_COMBO_ACTION = "custom_combo";
export const COMBO_ACTION_PREFIX = "combo:";

const MODIFIER_ORDER = [
  "lwin",
  "rwin",
  "lctrl",
  "rctrl",
  "lalt",
  "ralt",
  "lshift",
  "rshift",
] as const;

const MODIFIER_ALIASES: Record<string, string> = {
  lwin: "lwin",
  rwin: "rwin",
  win: "lwin",
  meta: "lwin",
  super: "lwin",
  lctrl: "lctrl",
  rctrl: "rctrl",
  ctrl: "lctrl",
  control: "lctrl",
  lalt: "lalt",
  ralt: "ralt",
  alt: "lalt",
  option: "lalt",
  lshift: "lshift",
  rshift: "rshift",
  shift: "lshift",
};

const MODIFIER_CODE_TO_TOKEN: Record<string, string> = {
  ControlLeft: "lctrl",
  ControlRight: "rctrl",
  ShiftLeft: "lshift",
  ShiftRight: "rshift",
  AltLeft: "lalt",
  AltRight: "ralt",
  MetaLeft: "lwin",
  MetaRight: "rwin",
  OSLeft: "lwin",
  OSRight: "rwin",
};

const CODE_TO_TOKEN: Record<string, string> = {
  Space: "space",
  Tab: "tab",
  Enter: "enter",
  NumpadEnter: "enter",
  Escape: "esc",
  Backspace: "backspace",
  Delete: "delete",
  Insert: "insert",
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  ContextMenu: "apps",
};

const NAMED_KEYS = new Set([
  "tab",
  "enter",
  "esc",
  "space",
  "backspace",
  "delete",
  "insert",
  "up",
  "down",
  "left",
  "right",
  "home",
  "end",
  "pageup",
  "pagedown",
  "apps",
]);

function isMainToken(tok: string): boolean {
  if (tok.length === 1 && /[a-z0-9]/.test(tok)) return true;
  if (/^f([1-9]|1[0-2])$/.test(tok)) return true;
  return NAMED_KEYS.has(tok);
}

const TOKEN_DISPLAY: Record<string, string> = {
  lwin: "左Win",
  rwin: "右Win",
  win: "左Win",
  lctrl: "左Ctrl",
  rctrl: "右Ctrl",
  ctrl: "左Ctrl",
  lalt: "左Alt",
  ralt: "右Alt",
  alt: "左Alt",
  lshift: "左Shift",
  rshift: "右Shift",
  shift: "左Shift",
  esc: "Esc",
  enter: "Enter",
  pageup: "PageUp",
  pagedown: "PageDown",
  backspace: "Backspace",
  delete: "Delete",
  insert: "Insert",
  space: "Space",
  apps: "Menu",
  tab: "Tab",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  home: "Home",
  end: "End",
};

export function modifierTokenFromCode(code: string): string | null {
  return MODIFIER_CODE_TO_TOKEN[code] ?? null;
}

function mainTokenFromEvent(e: KeyboardEvent): string | null {
  const code = e.code;
  if (code.startsWith("Key") && code.length === 4) {
    return code.slice(3).toLowerCase();
  }
  if (code.startsWith("Digit") && code.length === 6) {
    return code.slice(5);
  }
  if (code.startsWith("Numpad") && /^Numpad\d$/.test(code)) {
    return code.slice(6);
  }
  if (/^F([1-9]|1[0-2])$/.test(code)) {
    return code.toLowerCase();
  }
  return CODE_TO_TOKEN[code] ?? null;
}

/** 规范化 token：左右修饰键分开，顺序为 Win → Ctrl → Alt → Shift（先左后右）。 */
export function canonicalizeCombo(tokens: string[]): string[] | null {
  const mods: string[] = [];
  const mains: string[] = [];
  for (const raw of tokens) {
    const t = raw.trim().toLowerCase();
    if (!t) continue;
    const aliased = MODIFIER_ALIASES[t];
    if (aliased) {
      if (!mods.includes(aliased)) mods.push(aliased);
      continue;
    }
    if (isMainToken(t)) {
      mains.push(t);
    } else {
      return null;
    }
  }
  if (mains.length > 1) return null;
  if (mains.length === 0 && mods.length === 0) return null;
  mods.sort(
    (a, b) =>
      (MODIFIER_ORDER as readonly string[]).indexOf(a) -
      (MODIFIER_ORDER as readonly string[]).indexOf(b),
  );
  return mains.length ? [...mods, mains[0]] : mods;
}

export function formatComboDisplay(tokens: string[]): string {
  if (!tokens.length) return "";
  return tokens
    .map((t) => {
      if (TOKEN_DISPLAY[t]) return TOKEN_DISPLAY[t];
      if (/^f([1-9]|1[0-2])$/.test(t) || t.length === 1) return t.toUpperCase();
      return t;
    })
    .join("+");
}

export function toComboActionKey(tokens: string[]): string {
  return `${COMBO_ACTION_PREFIX}${tokens.join("+")}`;
}

export function parseComboActionKey(actionKey: string): string[] | null {
  if (!actionKey.startsWith(COMBO_ACTION_PREFIX)) return null;
  const spec = actionKey.slice(COMBO_ACTION_PREFIX.length);
  if (!spec) return null;
  return canonicalizeCombo(spec.split("+"));
}

/** 一次按键对应哪个 token：修饰键或主键；不支持的按键返回 null。 */
export function tokenFromEvent(e: KeyboardEvent): string | null {
  return modifierTokenFromCode(e.code) ?? mainTokenFromEvent(e);
}

/**
 * 由「当前按住的全部按键」拼出组合键。
 *
 * 录制时不能用 `e.ctrlKey` 判断修饰键（分不出左右），所以账本由调用方按键的
 * 按下/松开顺序维护。这里**不关心按下顺序**：修饰键归修饰键、主键取最后一个，
 * 所以「先按 Ctrl 再按 K」和「先按 K 再按 Ctrl」得到同一个 `lctrl+k`——人手
 * 同时按下两个键时操作系统给出的先后是随机的，录不出来才是 bug。
 *
 * 只按修饰键也是合法组合（`rctrl`），后端 `parse_combo_spec` 明确支持
 * 「修饰键可单独使用」，所以这里不做「必须有主键」的限制。
 */
export function comboFromHeld(held: string[]): string[] | null {
  const mods: string[] = [];
  let main: string | null = null;
  for (const raw of held) {
    const t = raw.trim().toLowerCase();
    if (!t) continue;
    const aliased = MODIFIER_ALIASES[t];
    if (aliased) {
      mods.push(aliased);
      continue;
    }
    main = t;
  }
  return canonicalizeCombo(main ? [...mods, main] : mods);
}
