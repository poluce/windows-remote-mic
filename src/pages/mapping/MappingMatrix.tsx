import { cellKey } from "./cells";
import type { MappingEntry, RemoteButton, TriggerOption } from "./types";

/**
 * 按键矩阵：行＝按键，列＝单击 / 双击 / 长按。
 *
 * 为什么不是平铺列表：列表只列得出「已经配了什么」，看不出「哪些还没绑」。
 * 矩阵把全部格子摊开，空格显示 `—`。
 *
 * 三种底色各有含义：**蓝底＝被当前这份配置覆盖过**，深色＝沿用全局，
 * 灰 `—` ＝ 未绑定。颜色是相对信息（「和全局不同」），光靠它猜不出来，
 * 所以蓝底那句说明由调用方通过 `hint` 传进来——只在应用作用域才有意义，
 * 全局作用域下整句不出现。
 */
export function MappingMatrix({
  title,
  hint,
  buttons,
  triggers,
  global,
  override,
  selected,
  onSelect,
}: {
  title: string;
  hint?: string;
  buttons: RemoteButton[];
  triggers: TriggerOption[];
  global: Map<string, MappingEntry>;
  override: Map<string, MappingEntry>;
  selected: { button: string; trigger: string };
  onSelect: (button: string, trigger: string) => void;
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
                const isSelected =
                  selected.button === button.key && selected.trigger === t.key;
                const cls = [
                  "matrix-cell",
                  own ? "own" : entry ? "on" : "",
                  isSelected ? "sel" : "",
                ]
                  .filter(Boolean)
                  .join(" ");

                return (
                  <span
                    key={t.key}
                    className={cls}
                    data-cell={key}
                    title={
                      own
                        ? `这份配置改过：${own.action}`
                        : entry
                          ? `沿用全局：${entry.action}`
                          : "未绑定"
                    }
                    onClick={() => onSelect(button.key, t.key)}
                  >
                    {entry?.action || "—"}
                  </span>
                );
              })}
            </div>
          );
        })}
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
