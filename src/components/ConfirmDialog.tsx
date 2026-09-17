import { useEffect } from "react";

/**
 * 二次确认对话框。
 *
 * 用来拦「删掉整份应用配置」这类不可撤销、又只有一次点击距离的操作。
 * 点遮罩或按 Esc 都算取消——危险操作的默认结果必须是「什么都没发生」。
 */
export function ConfirmDialog({
  open,
  title,
  desc,
  confirmLabel = "确定",
  danger = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  desc?: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  return (
    <div className="dialog-mask open" onClick={onCancel}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>{title}</h3>
        {desc && <p className="hint dialog-desc">{desc}</p>}
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className={`btn${danger ? " danger" : " primary"}`}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
