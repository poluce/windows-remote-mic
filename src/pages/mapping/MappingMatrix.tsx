import { cellKey } from "./cells";
import type { MappingEntry, RemoteButton, TriggerOption } from "./types";

/**
 * 按键矩阵：行＝按键，列＝单击 / 双击 / 长按。
 *
 * 为什么不是平铺列表：列表只列得出「已经配了什么」，看不出「哪些还没绑」。
 * 矩阵把全部格子摊开，空格显示 `—`。
 *
 * 格子状态：
 * - **已绑定（启用）**：文字微绿；悬停 / 选中时右上角出现红色 ×；
 * - **已禁用**：红色填充（动作保留，点格子恢复）；
 * - **未绑定**：`—`，点击选中后在左侧选动作保存；
 * - **应用档案覆盖**：蓝底（沿用全局的格子不带蓝底）。
 *
 * 交互（仅全局作用域开启，`interactive`）：
 * - 点击已绑定格 = 切换禁用 / 启用，并把它设为编辑目标；
 * - 点击空格 = 只选中，不切换状态；
 * - 点右上角 × = 取消关联（删除绑定，`onClear`）。
 */
export function MappingMatrix({
  title,
  hint,
  buttons,
  triggers,
  global,
  override,
  selected,
  interactive = false,
  onSelect,
  onCellClick,
  onClear,
}: {
  title: string;
  hint?: string;
  buttons: RemoteButton[];
  triggers: TriggerOption[];
  global: Map<string, MappingEntry>;
  override: Map<string, MappingEntry>;
  selected: { button: string; trigger: string };
  /** 全局作用域开启矩阵直接操作；应用档案作用域保持「点击选中」。 */
  interactive?: boolean;
  onSelect: (button: string, trigger: string) => void;
  /** 点击格子：bound 表示这一格当前有没有绑定。 */
  onCellClick?: (button: string, trigger: string, bound: boolean) => void;
  /** 点击右上角 ×：取消关联。 */
  onClear?: (button: string, trigger: string) => void;
}) {
  // 麦克风是 PTT 键，只有按下 / 松开，塞进三个触发列会误导。
  // 后端也不允许任何应用配置覆盖它（语音是全局语义），所以它永远读全局。
  const micPress = global.get(cellKey("mic", "press"));
  const micRelease = global.get(cellKey("mic", "release"));
  const micText = describeMic(micPress, micRelease);

  return (
    <>
      <div className="wizard-label">{title}</div>
      {hint && <p className="hint matrix-hint">{hint}</p>}

      <div className="matrix">
        <div className="matrix-row head">
          <span className="matrix-key">按键</span>
          {triggers.map((t) => (
            <span className="matrix-cell" key={t.key}>
              {t.label}
            </span>
          ))}
        </div>

        {buttons.map((button) => {
          if (button.key === "mic") {
            const micSelected = selected.button === button.key;
            return (
              <div className="matrix-row mic" key={button.key}>
                <span className="matrix-key">{button.name}</span>
                <span
                  className={`matrix-cell span${micSelected ? " sel" : ""}`}
                  data-cell="mic"
                  title={micText}
                  onClick={() => onSelect(button.key, "press")}
                >
                  {micText}
                </span>
              </div>
            );
          }

          return (
            <div className="matrix-row" key={button.key}>
              <span className="matrix-key">{button.name}</span>
              {triggers.map((t) => {
                const key = cellKey(button.key, t.key);
                const own = override.get(key);
                const entry = own ?? global.get(key);
                const enabled = entry?.enabled !== false;
                const isSelected =
                  selected.button === button.key && selected.trigger === t.key;
                const cls = [
                  "matrix-cell",
                  entry ? (enabled ? "on" : "off") : "empty",
                  own ? "own" : "",
                  isSelected ? "sel" : "",
                ]
                  .filter(Boolean)
                  .join(" ");

                const title = entry
                  ? `${own ? "这份配置改过：" : "沿用全局："}${entry.action}${
                      enabled ? "（点击禁用）" : "（已禁用，点击恢复）"
                    }`
                  : "未绑定（点击选中，再选动作保存）";

                return (
                  <span
                    key={t.key}
                    className={cls}
                    data-cell={key}
                    title={title}
                    onClick={() => {
                      if (interactive && onCellClick) {
                        onCellClick(button.key, t.key, Boolean(entry));
                      } else {
                        onSelect(button.key, t.key);
                      }
                    }}
                  >
                    <span className="matrix-cell-content">
                      <span className="matrix-cell-label">
                        {entry?.action || "—"}
                      </span>
                      {interactive && entry && (
                        <button
                          type="button"
                          className="matrix-cell-x"
                          title={`取消关联：${entry.action}`}
                          aria-label={`取消关联 ${button.name} · ${t.label}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            onClear?.(button.key, t.key);
                          }}
                        >
                          ×
                        </button>
                      )}
                    </span>
                  </span>
                );
              })}
            </div>
          );
        })}
      </div>

      <div className="matrix-legend">
        <span className="lg">
          <i className="sw on" />
          已绑定（文字微绿）
        </span>
        <span className="lg">
          <i className="sw off" />
          已禁用（点击恢复）
        </span>
        <span className="lg">
          <i className="sw empty" />
          未绑定
        </span>
        {interactive && (
          <span className="lg">
            <i className="sw x">×</i>
            取消关联（悬停 / 选中显示）
          </span>
        )}
      </div>
    </>
  );
}

/** 把麦克风的按下 / 松开两条绑成一行人话。 */
function describeMic(
  press: MappingEntry | undefined,
  release: MappingEntry | undefined,
): string {
  if (press && release && press.action === release.action) {
    return `按下 / 松开 → ${press.action}`;
  }
  const parts: string[] = [];
  if (press) parts.push(`按下 → ${press.action}`);
  if (release) parts.push(`松开 → ${release.action}`);
  return parts.length ? parts.join("　") : "未绑定";
}
