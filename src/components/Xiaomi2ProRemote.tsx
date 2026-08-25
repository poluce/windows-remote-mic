import "./Xiaomi2ProRemote.css";

type Props = {
  selected: string;
  onSelect: (key: string) => void;
};

const DIRECTION_HINTS: Record<string, string> = {
  up: "上",
  left: "左",
  ok: "确定",
  right: "右",
  down: "下",
};

// 90° 扇形边框：外径 42px、内径 20px，以对应方向为中心
const FAN_PATHS: Record<string, string> = {
  up: "M 12.30 12.30 A 42 42 0 0 1 71.70 12.30 L 56.14 27.86 A 20 20 0 0 0 27.86 27.86 Z",
  right:
    "M 71.70 12.30 A 42 42 0 0 1 71.70 71.70 L 56.14 56.14 A 20 20 0 0 0 56.14 27.86 Z",
  down: "M 71.70 71.70 A 42 42 0 0 1 12.30 71.70 L 27.86 56.14 A 20 20 0 0 0 56.14 56.14 Z",
  left: "M 12.30 71.70 A 42 42 0 0 1 12.30 12.30 L 27.86 27.86 A 20 20 0 0 0 27.86 56.14 Z",
};

export function Xiaomi2ProRemote({ selected, onSelect }: Props) {
  return (
    <div className="x2pro-remote">
      <div className="top-row">
        <button
          type="button"
          className={`top-btn remote-key ${selected === "power" ? "active" : ""}`}
          title="电源"
          onClick={() => onSelect("power")}
        >
          <div className="power-icon" />
        </button>
        <button
          type="button"
          className={`top-btn remote-key ${selected === "mic" ? "active" : ""}`}
          title="麦克风"
          onClick={() => onSelect("mic")}
        >
          <div className="mic-icon" />
        </button>
      </div>

      <div className="dpad">
        {(["up", "left", "right", "down"] as const).map((key) => (
          <button
            type="button"
            key={key}
            className={`dpad-hit hit-${key} ${selected === key ? "active" : ""}`}
            title={DIRECTION_HINTS[key]}
            onClick={() => onSelect(key)}
          >
            {selected === key && (
              <svg viewBox="0 0 84 84" className="fan-frame" aria-hidden="true">
                <path d={FAN_PATHS[key]} />
              </svg>
            )}
          </button>
        ))}
        <button
          type="button"
          className={`dpad-hit hit-ok ${selected === "ok" ? "active" : ""}`}
          title={DIRECTION_HINTS.ok}
          onClick={() => onSelect("ok")}
        />
      </div>

      <div className="bottom-grid">
        <div className="left-col">
          <button
            type="button"
            className={`small-btn remote-key ${selected === "back" ? "active" : ""}`}
            title="返回"
            onClick={() => onSelect("back")}
          >
            <div className="icon-back" />
          </button>
          <button
            type="button"
            className={`small-btn remote-key ${selected === "home" ? "active" : ""}`}
            title="主页"
            onClick={() => onSelect("home")}
          >
            <div className="icon-home" />
          </button>
          <button
            type="button"
            className={`small-btn remote-key ${selected === "menu" ? "active" : ""}`}
            title="菜单"
            onClick={() => onSelect("menu")}
          >
            <div className="icon-menu" />
          </button>
        </div>
        <div className="right-col">
          <div className="pill-volume">
            <button
              type="button"
              className={`vol-btn plus remote-key ${selected === "volume_up" ? "active" : ""}`}
              title="音量 +"
              onClick={() => onSelect("volume_up")}
            />
            <button
              type="button"
              className={`vol-btn minus remote-key ${selected === "volume_down" ? "active" : ""}`}
              title="音量 −"
              onClick={() => onSelect("volume_down")}
            />
          </div>
          <button
            type="button"
            className={`small-btn remote-key ${selected === "tv" ? "active" : ""}`}
            title="TV"
            onClick={() => onSelect("tv")}
          >
            <div className="icon-source" />
          </button>
        </div>
      </div>

      <div className="logo">xiaomi</div>
    </div>
  );
}
