const BUTTONS: { key: string; name: string; action: string }[] = [
  { key: "up", name: "上", action: "↑ 方向" },
  { key: "down", name: "下", action: "↓ 方向" },
  { key: "left", name: "左", action: "← 方向" },
  { key: "right", name: "右", action: "→ 方向" },
  { key: "ok", name: "确定", action: "Enter" },
  { key: "back", name: "返回", action: "Backspace" },
  { key: "home", name: "主页", action: "显示桌面 Win+D" },
  { key: "menu", name: "菜单", action: "上下文菜单" },
  { key: "tv", name: "TV", action: "应用切换 Alt+Tab" },
  { key: "power", name: "电源", action: "Esc" },
  { key: "vol_up", name: "音量 +", action: "系统音量增加" },
  { key: "vol_down", name: "音量 −", action: "系统音量减小" },
  { key: "mic", name: "麦克风", action: "语音输入（Win+H）" },
];

export function MappingPage() {
  return (
    <div className="page">
      <h2>按键映射</h2>
      <p className="page-sub">13 个按键（12 个普通键 + 1 个麦克风键）的当前动作。</p>

      <section className="card">
        <div className="card-title">默认映射</div>
        <div className="mapping-list">
          {BUTTONS.map((b) => (
            <div key={b.key} className="mapping-row">
              <span className="mapping-key">{b.name}</span>
              <span className="mapping-action">{b.action}</span>
              <button className="btn small" disabled>
                编辑
              </button>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
