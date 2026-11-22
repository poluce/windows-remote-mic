/**
 * 作用范围选择器：全局 + 每一份应用配置。
 *
 * 应用配置的胶囊上挂两个角标：**✎ 编辑**（改名 / 改匹配规则 / 改内环颜色）和
 * **× 删除**（整份配置回到全局）。编辑走和「新建应用」同一张弹窗，不再在
 * 条下面展开设置行。
 *
 * 胶囊本身就是这份配置的颜色：有了颜色就整颗填充（不再画一个小圆点），
 * 选中态用一圈与字色同源的描边表示。全局没有颜色，保持中性样式。
 */
import { readableTextColor } from "./iconColor";

export type ScopeItem = {
  id: string;
  label: string;
  /** 这份配置的内环颜色；有值时整颗胶囊用它填充，字色按亮度取黑/白。 */
  color?: string;
  removable: boolean;
};

export function ScopeBar({
  items,
  active,
  onSelect,
  onEdit,
  onRemove,
  onNew,
}: {
  items: ScopeItem[];
  active: string;
  onSelect: (id: string) => void;
  onEdit: (item: ScopeItem) => void;
  onRemove: (item: ScopeItem) => void;
  onNew: () => void;
}) {
  return (
    <div className="scope-bar">
      {items.map((item) => {
        const filled = Boolean(item.color);
        const pill = (
          <button
            type="button"
            className={[
              "scope-pill",
              filled ? "filled" : "",
              item.id === active ? "active" : "",
            ]
              .filter(Boolean)
              .join(" ")}
            style={
              item.color
                ? {
                    background: item.color,
                    color: readableTextColor(item.color),
                    borderColor: "transparent",
                  }
                : undefined
            }
            onClick={() => onSelect(item.id)}
          >
            {/* 有颜色时整颗胶囊就是那个颜色，再画圆点反而重复 */}
            {!filled && <span className="scope-dot" />}
            {item.label}
          </button>
        );

        if (!item.removable) {
          return <span key={item.id}>{pill}</span>;
        }
        return (
          <span className="scope-item" key={item.id}>
            {pill}
            <button
              type="button"
              className="scope-edit"
              title="编辑这份配置"
              aria-label={`编辑「${item.label}」的配置`}
              onClick={(e) => {
                e.stopPropagation();
                onEdit(item);
              }}
            >
              ✎
            </button>
            <button
              type="button"
              className="scope-remove"
              title="删除这份配置"
              aria-label={`删除「${item.label}」的配置`}
              onClick={(e) => {
                e.stopPropagation();
                onRemove(item);
              }}
            >
              ×
            </button>
          </span>
        );
      })}

      <button type="button" className="scope-pill new" onClick={onNew}>
        + 新建应用
      </button>
    </div>
  );
}
