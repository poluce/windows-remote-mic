import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type StatsSummary = {
  today_key_presses: number;
  today_voice_seconds: number;
  total_key_presses: number;
  total_voice_seconds: number;
};

type StatsDay = {
  day: string;
  key_presses: number;
  voice_seconds: number;
};

const EMPTY: StatsSummary = {
  today_key_presses: 0,
  today_voice_seconds: 0,
  total_key_presses: 0,
  total_voice_seconds: 0,
};

export function StatsPage() {
  const [stats, setStats] = useState<StatsSummary>(EMPTY);
  const [history, setHistory] = useState<StatsDay[]>([]);

  async function refresh() {
    if (!isTauri()) return;
    try {
      setStats(await invoke<StatsSummary>("get_stats_summary"));
      setHistory(await invoke<StatsDay[]>("get_stats_history"));
    } catch {
      // ignore
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function demoKey() {
    if (!isTauri()) return;
    try {
      setStats(await invoke<StatsSummary>("demo_record_key"));
      await refresh();
    } catch (err) {
      console.error(err);
    }
  }

  const max = Math.max(1, ...history.map((h) => h.key_presses));

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

      <section className="card">
        <div className="card-title">近 7 日按键次数</div>
        {history.length === 0 ? (
          <p className="hint">暂无历史数据。</p>
        ) : (
          <div className="bar-chart">
            {history.map((h) => (
              <div key={h.day} className="bar-col">
                <div
                  className="bar"
                  style={{ height: `${Math.max(4, (h.key_presses / max) * 120)}px` }}
                />
                <span className="bar-day">{h.day.replace(/^day/, "")}</span>
                <span className="bar-value">{h.key_presses}</span>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="card actions">
        <button className="btn" onClick={demoKey}>
          模拟一次按键（验证统计写入）
        </button>
      </section>
    </div>
  );
}
