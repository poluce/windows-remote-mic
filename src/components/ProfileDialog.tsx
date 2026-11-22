import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

import { loadIconPalette } from "../pages/mapping/profile";
import {
  hexToHsl,
  hslToHex,
  previewLabel,
} from "../pages/mapping/iconColor";

/** `app_profile_foreground_candidates` 返回的一项：一个最近出现过的前台应用。 */
type ForegroundCandidate = {
  process: string;
  title: string | null;
};

/** 表单提交出来的一份配置。`iconColor` 为 null 表示内环颜色走自动规则。 */
export type ProfileFormValues = {
  name: string;
  process: string;
  titleContains: string;
  iconColor: string | null;
};

/** 自定义取色的初始值（还没拖过滑杆时停在这里）。 */
const DEFAULT_CUSTOM_COLOR = "#4d6bfe";

/** 进程名去掉 `.exe` 后缀，用作展示名的默认值（`ZCode.exe` → `ZCode`）。 */
function processStem(process: string): string {
  return process.replace(/\.exe$/i, "");
}

/**
 * 应用配置表单：**新建与编辑共用**同一张弹窗。
 *
 * 两件事（认应用、定外观）按顺序排：展示名 + 进程名 → 内环颜色（收在展示名左侧
 * 圆圈里，点开是浮层）；「按窗口标题匹配」是少数情况才需要的高级项，默认收起。
 *
 * # 为什么不提供「当前前台」
 *
 * 用户是点本页的「新建 / 编辑应用」进来的，此刻前台窗口必然是 Remote Mic 自己，
 * 「当前前台」填进去只会得到 `remote-mic.exe`——一份只匹配本应用、永远命中
 * 不了目标应用的死配置。真正要填的是「刚才还在用、现在被本应用盖住」的那个
 * 应用，所以给的是后端记录的最近前台历史（已排除本应用自己）。
 */
export function ProfileDialog({
  open,
  mode,
  initial,
  onCancel,
  onSubmit,
}: {
  open: boolean;
  mode: "create" | "edit";
  /** 编辑时的初始值；新建时传 null。 */
  initial: ProfileFormValues | null;
  onCancel: () => void;
  onSubmit: (values: ProfileFormValues) => void;
}) {
  const [name, setName] = useState("");
  const [process, setProcess] = useState("");
  const [title, setTitle] = useState("");
  const [iconColor, setIconColor] = useState<string | null>(null);
  const [candidates, setCandidates] = useState<ForegroundCandidate[]>([]);
  const [palette, setPalette] = useState<string[]>([]);
  /** 「按窗口标题匹配」默认收起：只有网页应用 / WSL 服务才用得上。 */
  const [titleOpen, setTitleOpen] = useState(false);
  /** 展示名左侧圆圈点开的颜色浮层。 */
  const [colorOpen, setColorOpen] = useState(false);

  // 每次打开都按「新建 / 编辑哪一份」重置，避免带上一轮的残留。
  useEffect(() => {
    if (!open) return;
    setName(initial?.name ?? "");
    setProcess(initial?.process ?? "");
    setTitle(initial?.titleContains ?? "");
    setIconColor(initial?.iconColor ?? null);
    setTitleOpen(Boolean(initial?.titleContains?.trim()));
    setColorOpen(false);
    void loadIconPalette().then(setPalette);
    if (!isTauri()) {
      setCandidates([{ process: "ZCode.exe", title: "ZCode" }]);
      return;
    }
    invoke<ForegroundCandidate[]>("app_profile_foreground_candidates")
      .then((list) => setCandidates(Array.isArray(list) ? list : []))
      .catch(() => setCandidates([]));
  }, [open, mode, initial]);

  useEffect(() => {
    if (!open) return;
    function onKey(e: KeyboardEvent) {
      if (e.key !== "Escape") return;
      // Esc 先收颜色浮层，再收对话框：别让一次 Esc 把整张表单也关掉。
      if (colorOpen) {
        setColorOpen(false);
        return;
      }
      onCancel();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel, colorOpen]);

  if (!open) return null;

  const editing = mode === "edit";
  const canSubmit = process.trim().length > 0 || title.trim().length > 0;
  const label = name.trim() ? previewLabel(name) : "";
  /** 当前颜色不在预设色卡里 = 用户用滑杆调出来的自定义色。 */
  const isCustomColor =
    iconColor !== null &&
    !palette.some((color) => color.toLowerCase() === iconColor.toLowerCase());
  // 滑杆回显：停在当前颜色上（自动时停在默认色，拖动即转成自定义色）
  const currentHsl = hexToHsl(iconColor ?? DEFAULT_CUSTOM_COLOR) ?? {
    h: 220,
    s: 70,
    l: 58,
  };
  const hueTrack =
    "linear-gradient(90deg, hsl(0 70% 58%), hsl(60 70% 58%), hsl(120 70% 58%), hsl(180 70% 58%), hsl(240 70% 58%), hsl(300 70% 58%), hsl(360 70% 58%))";
  const lightTrack = `linear-gradient(90deg, #111, hsl(${currentHsl.h} ${currentHsl.s}% 50%), #fff)`;

  function applyHsl(h: number, s: number, l: number) {
    setIconColor(hslToHex(h, s, l));
  }

  /** 点最近前台：填进程名；展示名还空着时顺手按进程名给个默认值。 */
  function useCandidate(candidate: ForegroundCandidate) {
    setProcess(candidate.process);
    if (!name.trim()) setName(processStem(candidate.process));
  }

  function submit() {
    onSubmit({
      name: name.trim(),
      process: process.trim(),
      titleContains: title.trim(),
      iconColor,
    });
  }

  return (
    <div className="dialog-mask open" onClick={onCancel}>
      <div
        className="dialog dialog-profile"
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{editing ? "编辑应用配置" : "新建应用配置"}</h3>

        <div className="field">
          <label htmlFor="np-name">展示名</label>
          <div className="field-avatar">
            <button
              type="button"
              className={`icon-preview${iconColor ? "" : " auto"}${
                label ? "" : " empty"
              }`}
              style={iconColor ? { background: iconColor } : undefined}
              title="设置快捷菜单内环颜色"
              aria-label="设置快捷菜单内环颜色"
              aria-expanded={colorOpen}
              onClick={() => setColorOpen((open) => !open)}
            >
              {label || "+"}
            </button>
            <input
              id="np-name"
              value={name}
              placeholder="例如：ZCode"
              onChange={(e) => setName(e.target.value)}
            />

            {colorOpen && (
              <>
                {/* 点浮层外面关掉；透明背板只吃掉这一次点击 */}
                <div
                  className="color-pop-backdrop"
                  onClick={() => setColorOpen(false)}
                />
                <div className="color-pop" role="dialog" aria-label="内环颜色">
                  <div className="color-pop-title">快捷菜单内环颜色</div>
                  <div className="icon-color-row">
                    <button
                      type="button"
                      className={`icon-swatch auto${
                        iconColor === null ? " active" : ""
                      }`}
                      title="按配置自动分配，同一个应用每次都一样"
                      onClick={() => {
                        setIconColor(null);
                        setColorOpen(false);
                      }}
                    >
                      自动
                    </button>
                    {palette.map((color) => (
                      <button
                        key={color}
                        type="button"
                        className={`icon-swatch${
                          iconColor?.toLowerCase() === color.toLowerCase()
                            ? " active"
                            : ""
                        }`}
                        style={{ background: color }}
                        title={color}
                        aria-label={`选择颜色 ${color}`}
                        onClick={() => {
                          setIconColor(color);
                          setColorOpen(false);
                        }}
                      />
                    ))}
                  </div>

                  {/* 自定义颜色：不弹二级窗口，直接在浮层里拖滑杆 */}
                  <div className="color-pop-divider" />
                  <div className="color-custom">
                    <div className="color-custom-head">
                      <span>自定义</span>
                      <span className="color-custom-value">
                        {isCustomColor && iconColor ? iconColor : "自动"}
                      </span>
                    </div>
                    <label className="color-slider">
                      <span>色相</span>
                      <input
                        type="range"
                        min={0}
                        max={360}
                        step={1}
                        value={currentHsl.h}
                        aria-label="色相"
                        style={{ background: hueTrack }}
                        onChange={(e) =>
                          applyHsl(
                            Number(e.target.value),
                            currentHsl.s,
                            currentHsl.l,
                          )
                        }
                      />
                    </label>
                    <label className="color-slider">
                      <span>明度</span>
                      <input
                        type="range"
                        min={18}
                        max={88}
                        step={1}
                        value={currentHsl.l}
                        aria-label="明度"
                        style={{ background: lightTrack }}
                        onChange={(e) =>
                          applyHsl(
                            currentHsl.h,
                            currentHsl.s,
                            Number(e.target.value),
                          )
                        }
                      />
                    </label>
                  </div>
                </div>
              </>
            )}
          </div>
        </div>

        <div className="field">
          <label htmlFor="np-proc">进程名</label>
          <input
            id="np-proc"
            value={process}
            placeholder="任务管理器里的名称，例如 ZCode.exe"
            onChange={(e) => setProcess(e.target.value)}
          />
          {candidates.length > 0 ? (
            <div className="suggest">
              <span className="suggest-label">最近用过</span>
              <div className="suggest-chips">
                {candidates.map((candidate) => (
                  <button
                    key={candidate.process}
                    type="button"
                    className={`suggest-chip${
                      process.trim().toLowerCase() ===
                      candidate.process.toLowerCase()
                        ? " active"
                        : ""
                    }`}
                    title={candidate.title ?? undefined}
                    onClick={() => useCandidate(candidate)}
                  >
                    {candidate.process}
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="tip">
              先切到目标应用、再回来这里，会列出最近用过的进程名。
            </div>
          )}
        </div>

        {titleOpen ? (
          <>
            <div className="dialog-divider" />
            <div className="field">
              <div className="field-label-row">
                <label htmlFor="np-title">窗口标题关键字</label>
                <button
                  type="button"
                  className="link-btn"
                  onClick={() => setTitleOpen(false)}
                >
                  收起
                </button>
              </div>
              <input
                id="np-title"
                value={title}
                placeholder="例如：DeepSeek Harness"
                onChange={(e) => setTitle(e.target.value)}
              />
              <div className="tip">
                用于进程名认不出来的目标（Chrome 网页应用、跑在 WSL 里的服务）。
                进程名命中时不会走它。
              </div>
            </div>
          </>
        ) : (
          <div className="dialog-more">
            <button
              type="button"
              className="link-btn"
              onClick={() => setTitleOpen(true)}
            >
              进程名认不出来？按窗口标题匹配
            </button>
          </div>
        )}

        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            type="button"
            className="btn primary"
            disabled={!canSubmit}
            onClick={submit}
          >
            {editing ? "保存" : "创建"}
          </button>
        </div>
      </div>
    </div>
  );
}
