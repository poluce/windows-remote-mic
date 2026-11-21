import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

/** `app_profile_foreground_candidates` 返回的一项：一个最近出现过的前台应用。 */
type ForegroundCandidate = {
  process: string;
  title: string | null;
};

/** 进程名去掉 `.exe` 后缀，用作展示名的默认值（`ZCode.exe` → `ZCode`）。 */
function processStem(process: string): string {
  return process.replace(/\.exe$/i, "");
}

/**
 * 新建一份应用配置。
 *
 * 只问三件事：展示名、进程名、可选的窗口标题关键字。第二个是这份配置能不能
 * 生效的关键，所以旁边直接给出**最近前台应用**并允许一键填入——让人去任务
 * 管理器里抄，抄错一个字母这份配置就永远不会命中，而且不会有任何报错。
 *
 * # 为什么不提供「当前前台」
 *
 * 用户是点本页的「+ 新建应用」进来的，此刻前台窗口必然是 Remote Mic 自己，
 * 「当前前台」填进去只会得到 `remote-mic.exe`——一份只匹配本应用、永远命中
 * 不了目标应用的死配置。真正要填的是「刚才还在用、现在被本应用盖住」的那个
 * 应用，所以后端给的是最近前台历史（已排除本应用自己）。
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
  const [candidates, setCandidates] = useState<ForegroundCandidate[]>([]);

  // 每次打开都重新读一次：对话框可能开着的时候用户又切过别的应用。
  useEffect(() => {
    if (!open) return;
    if (!isTauri()) {
      setCandidates([{ process: "ZCode.exe", title: "ZCode" }]);
      return;
    }
    invoke<ForegroundCandidate[]>("app_profile_foreground_candidates")
      .then((list) => setCandidates(Array.isArray(list) ? list : []))
      .catch(() => setCandidates([]));
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

  /** 点最近前台：填进程名；展示名还空着时顺手按进程名给个默认值。 */
  function useCandidate(candidate: ForegroundCandidate) {
    setProcess(candidate.process);
    if (!name.trim()) setName(processStem(candidate.process));
  }

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
          </div>
          <div className="tip">
            在任务管理器「详细信息」里查到的名称。写错了这份配置不会生效。
          </div>
          {candidates.length > 0 ? (
            <div className="tip">
              最近前台（点一下填入进程名）：
              <span className="fg-chips">
                {candidates.map((candidate) => (
                  <button
                    key={candidate.process}
                    type="button"
                    className="btn ghost small"
                    title={candidate.title ?? undefined}
                    onClick={() => useCandidate(candidate)}
                  >
                    {candidate.process}
                  </button>
                ))}
              </span>
            </div>
          ) : (
            <div className="tip">
              先切到目标应用、再回到本页新建，这里就会列出最近用过的进程名。
            </div>
          )}
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
