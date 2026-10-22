//! 组合快捷键规范化、解析与显示格式化。
//!
//! 组合键规范：形如 `ctrl+c`、`win+h`、`alt+f4`。
//! 修饰键顺序标准化为 win → ctrl → alt → shift，主键最多一个。

/// 自定义快捷键动作的稳定前缀，前端 `action_key` 形如 `combo:ctrl+c`。
pub const COMBO_ACTION_PREFIX: &str = "combo:";

const MODIFIER_ORDER: [&str; 8] = [
    "lwin", "rwin", "lctrl", "rctrl", "lalt", "ralt", "lshift", "rshift",
];

fn canonical_combo_token(tok: &str) -> Option<String> {
    let t = tok.trim().to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    Some(match t.as_str() {
        "lwin" => "lwin".into(),
        "rwin" => "rwin".into(),
        "win" | "meta" | "super" => "lwin".into(),
        "lctrl" => "lctrl".into(),
        "rctrl" => "rctrl".into(),
        "ctrl" | "control" => "lctrl".into(),
        "lalt" => "lalt".into(),
        "ralt" => "ralt".into(),
        "alt" | "option" => "lalt".into(),
        "lshift" => "lshift".into(),
        "rshift" => "rshift".into(),
        "shift" => "lshift".into(),
        "esc" | "escape" => "esc".into(),
        "enter" | "return" => "enter".into(),
        "pgup" | "pageup" | "page_up" => "pageup".into(),
        "pgdn" | "pagedown" | "page_down" => "pagedown".into(),
        "del" | "delete" => "delete".into(),
        "ins" | "insert" => "insert".into(),
        "bs" | "backspace" => "backspace".into(),
        "spc" | "space" => "space".into(),
        "apps" | "context_menu" => "apps".into(),
        "tab" | "up" | "down" | "left" | "right" | "home" | "end" => t,
        other if is_letter_token(other) || is_digit_token(other) || is_fn_token(other) => {
            other.to_string()
        }
        _ => return None,
    })
}

fn is_letter_token(tok: &str) -> bool {
    matches!(tok.chars().next(), Some(c) if tok.len() == 1 && c.is_ascii_lowercase())
}

fn is_digit_token(tok: &str) -> bool {
    matches!(tok.chars().next(), Some(c) if tok.len() == 1 && c.is_ascii_digit())
}

fn is_fn_token(tok: &str) -> bool {
    let rest = match tok.strip_prefix('f') {
        Some(r) => r,
        None => return false,
    };
    rest.parse::<u8>()
        .ok()
        .is_some_and(|n| (1..=12).contains(&n))
}

fn is_modifier_token(tok: &str) -> bool {
    matches!(
        tok,
        "lwin" | "rwin" | "lctrl" | "rctrl" | "lalt" | "ralt" | "lshift" | "rshift"
    )
}

/// 解析用户快捷键（`ctrl` / `ctrl+c` / `Win+H`），得到规范 token 序列。
///
/// 规则：至少一个键；主键最多一个；修饰键可单独使用（如只按 Ctrl）。
/// 修饰键顺序固定为 win → ctrl → alt → shift。未知键或空输入返回 None。
pub fn parse_combo_spec(spec: &str) -> Option<Vec<String>> {
    let raw: Vec<String> = spec
        .split('+')
        .map(canonical_combo_token)
        .collect::<Option<Vec<_>>>()?;
    if raw.is_empty() {
        return None;
    }

    let mut modifiers = Vec::new();
    let mut mains = Vec::new();
    for tok in raw {
        if is_modifier_token(&tok) {
            if modifiers.contains(&tok) {
                return None;
            }
            modifiers.push(tok);
        } else {
            mains.push(tok);
        }
    }
    if mains.len() > 1 {
        return None;
    }
    if mains.is_empty() && modifiers.is_empty() {
        return None;
    }
    modifiers.sort_by_key(|m| {
        MODIFIER_ORDER
            .iter()
            .position(|x| x == m)
            .unwrap_or(MODIFIER_ORDER.len())
    });
    if let Some(main) = mains.pop() {
        modifiers.push(main);
    }
    Some(modifiers)
}

/// `combo:ctrl+c` → token 列表；非该前缀返回 None。
pub fn parse_combo_action_key(action_key: &str) -> Option<Vec<String>> {
    parse_combo_spec(action_key.strip_prefix(COMBO_ACTION_PREFIX)?)
}

/// 前端 / 配置使用的稳定动作标识，如 `combo:ctrl+shift+s`。
pub fn combo_action_key(tokens: &[String]) -> String {
    format!("{COMBO_ACTION_PREFIX}{}", tokens.join("+"))
}

/// 展示用组合键文案，如 `Ctrl+Shift+S`。
pub fn combo_display(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| combo_token_display(t))
        .collect::<Vec<_>>()
        .join("+")
}

fn combo_token_display(tok: &str) -> String {
    match tok {
        "lwin" | "win" => "左Win".into(),
        "rwin" => "右Win".into(),
        "lctrl" | "ctrl" => "左Ctrl".into(),
        "rctrl" => "右Ctrl".into(),
        "lalt" | "alt" => "左Alt".into(),
        "ralt" => "右Alt".into(),
        "lshift" | "shift" => "左Shift".into(),
        "rshift" => "右Shift".into(),
        "esc" => "Esc".into(),
        "enter" => "Enter".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "insert" => "Insert".into(),
        "space" => "Space".into(),
        "apps" => "Menu".into(),
        "tab" => "Tab".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        "left" => "←".into(),
        "right" => "→".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        other if is_fn_token(other) => other.to_ascii_uppercase(),
        other if other.len() == 1 => other.to_ascii_uppercase(),
        other => other.to_string(),
    }
}
