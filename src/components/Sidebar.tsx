import { invoke, isTauri } from "@tauri-apps/api/core";

export type PageId = "connection" | "mapping" | "voice" | "diagnostics" | "guidance" | "stats";

const NAV_ITEMS: { id: PageId; label: string }[] = [
  { id: "connection", label: "连接" },
  { id: "mapping", label: "按键映射" },
  { id: "voice", label: "语音" },
  { id: "diagnostics", label: "诊断" },
  { id: "guidance", label: "引导" },
  { id: "stats", label: "统计" },
];

export function Sidebar({
  page,
  onChange,
}: {
  page: PageId;
  onChange: (page: PageId) => void;
}) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-logo">🎤</span>
        <div>
          <div className="brand-name">Remote Mic</div>
          <div className="brand-sub">Windows 无线麦</div>
        </div>
      </div>

      <nav className="nav">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.id}
            className={page === item.id ? "nav-item active" : "nav-item"}
            onClick={() => onChange(item.id)}
          >
            <span>{item.label}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-footer">
        <button
          className="btn small"
          onClick={() => {
            if (isTauri()) invoke("toggle_quick_menu");
          }}
        >
          快捷菜单
        </button>
      </div>
    </aside>
  );
}
