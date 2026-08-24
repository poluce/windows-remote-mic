import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type StatsSummary = {
  today_key_presses: number;
  today_voice_seconds: number;
  total_key_presses: number;
  total_voice_seconds: number;
};

const EMPTY: StatsSummary = {
  today_key_presses: 0,
  today_voice_seconds: 0,
  total_key_presses: 0,
  total_voice_seconds: 0,
};

export function StatsPage() {
  const [stats, setStats] = useState<StatsSummary>(EMPTY);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<StatsSummary>("get_stats_summary")
      .then(setStats)
      .catch(() => {});
  }, []);

  async function demoKey() {
    if (!isTauri()) return;
    try {
      setStats(await invoke<StatsSummary>("demo_record_key"));
    } catch (err) {
      console.error(err);
    }
  }

  return (
    <div className="page">
      <h2>统计</h2>
      <p className="page-sub">按键次数与语音时长仅保存在本机，不上传。</p>

      <section className="card">
        <div className="card-title">今日</div>
        <div className="brief-grid">
          <div className="brief">
            <div className="brief-value">{stats.today_key_presses}</div>
            <div className="brief-label">按键次数</div>
          </div>
          <div className="brief">
            <div className="brief-value">{stats.today_voice_seconds}s</div>
            <div className="brief-label">语音时长</div>
          </div>
        </div>
      </section>

      <section className="card">
        <div className="card-title">累计</div>
        <div className="brief-grid">
          <div className="brief">
            <div className="brief-value">{stats.total_key_presses}</div>
            <div className="brief-label">按键次数</div>
          </div>
          <div className="brief">
            <div className="brief-value">{stats.total_voice_seconds}s</div>
            <div className="brief-label">语音时长</div>
          </div>
        </div>
      </section>

      <section className="card actions">
        <button className="btn" onClick={demoKey}>
          模拟一次按键（验证统计写入）
        </button>
      </section>
    </div>
  );
}
