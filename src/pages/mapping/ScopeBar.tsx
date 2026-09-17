/**
 * 作用范围选择器：全局 + 每一份应用配置。
 *
 * 「除了全局，其余都能删」——所以 × 只长在应用配置上。它删的是**整份配置**
 * （那个应用的所有按键回到全局），和向导里的「改用全局」（只撤掉选中的那一格）
 * 是两件事。
 */
export type ScopeItem = {
  id: string;
  label: string;
  /** 品牌色小圆点；不传则用默认灰点。 */
  color?: string;
  removable: boolean;
};

export function ScopeBar({
  items,
  active,
  onSelect,
  onRemove,
  onNew,
}: {
  items: ScopeItem[];
  active: string;
  onSelect: (id: string) => void;
  onRemove: (item: ScopeItem) => void;
  onNew: () => void;
}) {
  return (
    <div className="scope-bar">
      {items.map((item) => {
        const pill = (
          <button
            type="button"
            className={`scope-pill${item.id === active ? " active" : ""}`}
            onClick={() => onSelect(item.id)}
          >
            <span
              className="scope-dot"
              style={item.color ? { background: item.color } : undefined}
            />
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
