import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

/** `app_profile_status` 里本对话框用得到的字段。 */
type ForegroundStatus = {
  process: string | null;
  title: string | null;
};

/**
 * 新建一份应用配置。
 *
 * 只问三件事：展示名、进程名、可选的窗口标题关键字。第二个是这份配置能不能
 * 生效的关键，所以旁边直接给出**当前前台进程名**并允许一键填入——让人去任务
 * 管理器里抄，抄错一个字母这份配置就永远不会命中，而且不会有任何报错。
 */
export function NewProfileDialog({
  open,
  onCancel,
  onCreate,
}: {
  open: boolean;
  onCancel: () => void;
  onCreate: (name: string, process: string, titleContains: string) => void;
}) {
  const [name, setName] = useState("");
  const [process, setProcess] = useState("");
  const [title, setTitle] = useState("");
  const [foreground, setForeground] = useState<ForegroundStatus | null>(null);

  // 每次打开都重新读一次前台：对话框可能开着的时候用户已经切走了。
  useEffect(() => {
    if (!open) return;
    if (!isTauri()) {
      setForeground({ process: "ZCode.exe", title: "ZCode" });
      return;
    }
    invoke<ForegroundStatus>("app_profile_status")
      .then(setForeground)
      .catch(() => setForeground(null));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  if (!open) return null;

  const canCreate = process.trim().length > 0 || title.trim().length > 0;
  const fgProcess = foreground?.process?.trim() || "";

  return (
    <div className="dialog-mask open" onClick={onCancel}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>新建应用配置</h3>
        <p className="hint dialog-desc">
          只为这一个应用覆盖需要改的按键，其余自动沿用全局。
        </p>

        <div className="field">
          <label htmlFor="np-name">展示名</label>
          <input
            id="np-name"
            value={name}
            placeholder="例如：ZCode"
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div className="field">
          <label htmlFor="np-proc">进程名</label>
          <div className="row">
            <input
              id="np-proc"
              value={process}
              placeholder="例如：ZCode.exe"
              onChange={(e) => setProcess(e.target.value)}
            />
            <button
              type="button"
              className="btn"
              disabled={!fgProcess}
              onClick={() => setProcess(fgProcess)}
            >
              用当前前台填入
            </button>
          </div>
          <div className="tip">
            在任务管理器「详细信息」里查到的名称。写错了这份配置不会生效。
            {fgProcess && (
              <>
                {" "}
                当前前台：<code>{fgProcess}</code>
              </>
            )}
          </div>
        </div>

        <div className="field">
          <label htmlFor="np-title">
            窗口标题关键字 <span className="stage2">可选</span>
          </label>
          <input
            id="np-title"
            value={title}
            placeholder="例如：DeepSeek Harness"
            onChange={(e) => setTitle(e.target.value)}
          />
          <div className="tip">
            用于<b>进程名认不出来</b>的目标：Chrome 里的网页应用、跑在 WSL 里的服务。
            进程名命中时不会走它。
          </div>
        </div>

        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="btn primary"
            disabled={!canCreate}
            onClick={() => onCreate(name, process, title)}
          >
            创建
          </button>
        </div>
      </div>
    </div>
  );
}
