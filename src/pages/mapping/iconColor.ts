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
