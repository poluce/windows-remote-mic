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

export function Xiaomi2ProRemote({ selected, onSelect }: Props) {
  const hit = (key: string) => ({
    type: "button" as const,
    className: `dpad-hit hit-${key} ${selected === key ? "active" : ""}`,
    title: DIRECTION_HINTS[key],
    onClick: () => onSelect(key),
  });

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
        <button {...hit("up")} />
        <button {...hit("left")} />
        <button {...hit("ok")} />
        <button {...hit("right")} />
        <button {...hit("down")} />
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
