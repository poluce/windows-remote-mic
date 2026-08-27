export type PageId = "connection" | "mapping" | "diagnostics" | "guidance";

const NAV_ITEMS: { id: PageId; label: string }[] = [
  { id: "connection", label: "连接" },
  { id: "mapping", label: "按键映射" },
  { id: "diagnostics", label: "诊断" },
  { id: "guidance", label: "引导" },
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
        <img src="/app-icon.svg" alt="Remote Mic" className="brand-logo" />
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

      </aside>
  );
}
