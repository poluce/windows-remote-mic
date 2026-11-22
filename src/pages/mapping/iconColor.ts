/**
 * 快捷菜单内环简标的展示层辅助。
 *
 * 真正的简标由后端算（`AppProfile::menu_icon`）：显式 `icon.label` 优先，否则按
 * 展示名取前 1–2 个字母/数字/汉字（ASCII 转大写），颜色没手挑时按配置 id 生成。
 * 这里复制同一套取字规则只是为了在「新建 / 换颜色」时立刻画一个预览，
 * 不参与任何持久化。
 */

/** 预览文字：展示名前 1–2 个字母/数字/汉字，取不到时用 `?`。 */
export function previewLabel(name: string): string {
  let label = "";
  for (const ch of name) {
    if (!/[\p{L}\p{N}]/u.test(ch)) continue;
    label += /[a-z]/i.test(ch) ? ch.toUpperCase() : ch;
    if ([...label].length >= 2) break;
  }
  return label || "?";
}


/** HSL 三元组；h ∈ [0,360)，s / l ∈ [0,100]。 */
export type Hsl = { h: number; s: number; l: number };

/**
 * `#rrggbb` → HSL。
 *
 * 只用于「自定义颜色」的滑杆回显：用户点开浮层时要让滑杆停在他当前的颜色上。
 * 解析失败返回 null，由调用方退回默认值。
 */
export function hexToHsl(hex: string): Hsl | null {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return null;
  const r = parseInt(match[1].slice(0, 2), 16) / 255;
  const g = parseInt(match[1].slice(2, 4), 16) / 255;
  const b = parseInt(match[1].slice(4, 6), 16) / 255;

  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const delta = max - min;

  let h = 0;
  let s = 0;
  if (delta !== 0) {
    s = l > 0.5 ? delta / (2 - max - min) : delta / (max + min);
    if (max === r) h = ((g - b) / delta) % 6;
    else if (max === g) h = (b - r) / delta + 2;
    else h = (r - g) / delta + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h: Math.round(h), s: Math.round(s * 100), l: Math.round(l * 100) };
}

/** HSL → `#rrggbb`（滑杆改动后写回颜色）。 */
export function hslToHex(h: number, s: number, l: number): string {
  const sat = Math.min(1, Math.max(0, s / 100));
  const light = Math.min(1, Math.max(0, l / 100));
  const c = (1 - Math.abs(2 * light - 1)) * sat;
  const hp = ((((h % 360) + 360) % 360) / 60);
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let rgb: [number, number, number];
  if (hp < 1) rgb = [c, x, 0];
  else if (hp < 2) rgb = [x, c, 0];
  else if (hp < 3) rgb = [0, c, x];
  else if (hp < 4) rgb = [0, x, c];
  else if (hp < 5) rgb = [x, 0, c];
  else rgb = [c, 0, x];

  const m = light - c / 2;
  const channel = (v: number) =>
    Math.round((v + m) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${channel(rgb[0])}${channel(rgb[1])}${channel(rgb[2])}`;
}


/**
 * 简标文字该用白字还是深色字。
 *
 * 预设色卡里既有 `#B84DFF` 这种深紫，也有 `#FFE600` 这种亮黄；统一白字会把亮色
 * 上的字糊掉。按 WCAG 的相对亮度算一下，选对比度更高的那一侧。
 */
export function readableTextColor(hex: string): "#ffffff" | "#131a24" {
  const match = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) return "#ffffff";
  const channel = (offset: number) => {
    const v = parseInt(match[1].slice(offset, offset + 2), 16) / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  const luminance =
    0.2126 * channel(0) + 0.7152 * channel(2) + 0.0722 * channel(4);
  // 与白色、深色的对比度比较：luminance 越低越该用白字。
  const contrastWhite = 1.05 / (luminance + 0.05);
  const contrastDark = (luminance + 0.05) / 0.058;
  return contrastWhite >= contrastDark ? "#ffffff" : "#131a24";
}
