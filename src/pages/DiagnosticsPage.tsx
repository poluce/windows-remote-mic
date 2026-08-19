const CHECKS = [
  { name: "蓝牙 / BLE + GATT", status: "待检测", tone: "warn" },
  { name: "虚拟声卡端点", status: "待检测", tone: "warn" },
  { name: "按键采集 / Raw Input", status: "待检测", tone: "warn" },
  { name: "系统语音键入 (Win+H)", status: "待检测", tone: "warn" },
];

export function DiagnosticsPage() {
  return (
    <div className="page">
      <h2>诊断</h2>
      <p className="page-sub">检查蓝牙、按键、虚拟声卡和系统语音链路。</p>

      <section className="card">
        <div className="card-title">检查项</div>
        <div className="check-list">
          {CHECKS.map((c) => (
            <div key={c.name} className="check-row">
              <span>{c.name}</span>
              <span className={`badge badge-${c.tone}`}>{c.status}</span>
            </div>
          ))}
        </div>
      </section>

      <section className="card actions">
        <button className="btn primary" disabled>
          运行检查
        </button>
        <button className="btn" disabled>
          查看日志
        </button>
      </section>
    </div>
  );
}
