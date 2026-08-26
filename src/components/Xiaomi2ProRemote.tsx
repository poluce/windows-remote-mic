import "./Xiaomi2ProRemote.css";

type Props = {
  selected?: string;
  targetKey?: string;
  doneKeys?: string[];
  onSelect?: (key: string) => void;
};

const DIRECTION_HINTS: Record<string, string> = {
  up: "上",
  left: "左",
  ok: "确定",
  right: "右",
  down: "下",
};

// 90° 扇形边框：外径 40px（留出描边空间）、内径 20px，以对应方向为中心
const FAN_PATHS: Record<string, string> = {
  up: "M 13.72 13.72 A 40 40 0 0 1 70.28 13.72 L 56.14 27.86 A 20 20 0 0 0 27.86 27.86 Z",
  right:
    "M 70.28 13.72 A 40 40 0 0 1 70.28 70.28 L 56.14 56.14 A 20 20 0 0 0 56.14 27.86 Z",
  down: "M 70.28 70.28 A 40 40 0 0 1 13.72 70.28 L 27.86 56.14 A 20 20 0 0 0 56.14 56.14 Z",
  left: "M 13.72 70.28 A 40 40 0 0 1 13.72 13.72 L 27.86 27.86 A 20 20 0 0 0 27.86 56.14 Z",
};

export function Xiaomi2ProRemote({ selected, targetKey, doneKeys, onSelect }: Props) {
  function getKeyClass(key: string, baseClass = "remote-key") {
    const isSel = selected === key;
    const isTarget = targetKey === key;
    const isDone = doneKeys?.includes(key);
    return `${baseClass} ${isSel ? "active" : ""} ${isTarget ? "guided-target" : ""} ${
      isDone ? "guided-done" : ""
    }`.trim();
  }

  function handleSelect(key: string) {
    if (onSelect) onSelect(key);
  }

  return (
    <div className="x2pro-remote">
      <div className="top-row">
        <button
          type="button"
          className={getKeyClass("power", "top-btn remote-key")}
          title="电源"
          onClick={() => handleSelect("power")}
        >
          <div className="power-icon" />
        </button>
        <button
          type="button"
          className={getKeyClass("mic", "top-btn remote-key")}
          title="麦克风"
          onClick={() => handleSelect("mic")}
        >
          <div className="mic-icon" />
        </button>
      </div>

      <div className="dpad">
        {(["up", "left", "right", "down"] as const).map((key) => {
          const isSel = selected === key;
          const isTarget = targetKey === key;
          const isDone = doneKeys?.includes(key);
          const showFan = isSel || isTarget || isDone;
          return (
            <button
              type="button"
              key={key}
              className={`dpad-hit hit-${key} ${isSel ? "active" : ""} ${
                isTarget ? "guided-target" : ""
              } ${isDone ? "guided-done" : ""}`.trim()}
              title={DIRECTION_HINTS[key]}
              onClick={() => handleSelect(key)}
            >
              {showFan && (
                <svg viewBox="0 0 84 84" className="fan-frame" aria-hidden="true">
                  <path d={FAN_PATHS[key]} />
                </svg>
              )}
            </button>
          );
        })}
        <button
          type="button"
          className={`dpad-hit hit-ok ${selected === "ok" ? "active" : ""} ${
            targetKey === "ok" ? "guided-target" : ""
          } ${doneKeys?.includes("ok") ? "guided-done" : ""}`.trim()}
          title={DIRECTION_HINTS.ok}
          onClick={() => handleSelect("ok")}
        />
      </div>

      <div className="bottom-grid">
        <div className="left-col">
          <button
            type="button"
            className={getKeyClass("back", "small-btn remote-key")}
            title="返回"
            onClick={() => handleSelect("back")}
          >
            <div className="icon-back" />
          </button>
          <button
            type="button"
            className={getKeyClass("home", "small-btn remote-key")}
            title="主页"
            onClick={() => handleSelect("home")}
          >
            <div className="icon-home" />
          </button>
          <button
            type="button"
            className={getKeyClass("menu", "small-btn remote-key")}
            title="菜单"
            onClick={() => handleSelect("menu")}
          >
            <div className="icon-menu" />
          </button>
        </div>
        <div className="right-col">
          <div className="pill-volume">
            <button
              type="button"
              className={getKeyClass("volume_up", "vol-btn plus remote-key")}
              title="音量 +"
              onClick={() => handleSelect("volume_up")}
            />
            <button
              type="button"
              className={getKeyClass("volume_down", "vol-btn minus remote-key")}
              title="音量 −"
              onClick={() => handleSelect("volume_down")}
            />
          </div>
          <button
            type="button"
            className={getKeyClass("tv", "small-btn remote-key")}
            title="TV"
            onClick={() => handleSelect("tv")}
          >
            <div className="icon-source" />
          </button>
        </div>
      </div>

      <div className="logo">xiaomi</div>
    </div>
  );
}
