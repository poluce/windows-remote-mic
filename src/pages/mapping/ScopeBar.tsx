/**
 * 作用范围选择器：全局 + 每一份应用配置。
 *
 * 应用配置的胶囊上挂两个角标：**✎ 编辑**（改名 / 改匹配规则 / 改内环颜色）和
 * **× 删除**（整份配置回到全局）。编辑走和「新建应用」同一张弹窗，不再在
 * 条下面展开设置行；× 删的是整份配置，和向导里的「改用全局」（只撤掉选中的
 * 那一格）是两件事。
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
