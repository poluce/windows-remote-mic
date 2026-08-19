const BRIEFS = [
  { label: "蓝牙", value: "待连接", tone: "warn" },
  { label: "设备", value: "RC003 / 2 Pro", tone: "ok" },
  { label: "ATVV 语音", value: "未启用", tone: "warn" },
];

export function ConnectionPage() {
  return (
    <div className="page">
      <h2>连接</h2>
      <p className="page-sub">连接小米蓝牙语音遥控器，让按键和语音进入 Windows。</p>

      <section className="card device-card">
        <div className="device-info">
          <span className="device-icon">📡</span>
          <div>
            <div className="device-name">小米蓝牙遥控器 2 Pro</div>
            <div className="device-model">RC003 · VID 0x2717 · PID 0x32B8</div>
          </div>
        </div>
        <span className="badge badge-warn">未连接</span>
      </section>

      <section className="card">
        <div className="card-title">状态概览</div>
        <div className="brief-grid">
          {BRIEFS.map((b) => (
            <div key={b.label} className={`brief ${b.tone}`}>
              <div className="brief-value">{b.value}</div>
              <div className="brief-label">{b.label}</div>
            </div>
          ))}
        </div>
      </section>

      <section className="card actions">
        <button className="btn primary" disabled>
          重新检测设备
        </button>
        <button className="btn" disabled>
          重新连接
        </button>
      </section>
    </div>
  );
}
