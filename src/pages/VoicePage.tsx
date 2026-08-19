export function VoicePage() {
  return (
    <div className="page">
      <h2>语音</h2>
      <p className="page-sub">
        优先使用 Windows 自带语音输入（Win+H），无需安装第三方输入法。
      </p>

      <section className="card">
        <div className="card-title">语音识别目标</div>
        <div className="voice-route">
          <div className="route-node active">
            <span>🎙️</span>
            <div>Windows 语音键入</div>
            <small>Win + H</small>
          </div>
          <span className="route-arrow">→</span>
          <div className="route-node">
            <span>🎧</span>
            <div>虚拟声卡</div>
            <small>CABLE Output</small>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-title">输出端点</div>
        <select className="select" defaultValue="cable-input" disabled>
          <option value="cable-input">CABLE Input (VB-CABLE)</option>
          <option value="default">系统默认输出</option>
        </select>
      </section>

      <section className="card actions">
        <button className="btn primary" disabled>
          发送 1 秒测试音
        </button>
        <button className="btn" disabled>
          打开系统语音设置
        </button>
      </section>
    </div>
  );
}
