import { tapStatusLabel, useRuntimeStatus } from "../store/runtimeStatus";
import { IconActivity, IconBluetooth, IconCompass, IconGrid, IconLog } from "./icons";

export type PageId = "connection" | "mapping" | "diagnostics" | "log" | "guidance";

const NAV_ITEMS: {
  id: PageId;
  label: string;
  icon: typeof IconBluetooth;
  color: string;
}[] = [
  { id: "connection", label: "连接", icon: IconBluetooth, color: "#007aff" },
  { id: "mapping", label: "按键映射", icon: IconGrid, color: "#af52de" },
  { id: "diagnostics", label: "诊断", icon: IconActivity, color: "#5e5ce6" },
  { id: "log", label: "日志", icon: IconLog, color: "#32ade6" },
  { id: "guidance", label: "引导", icon: IconCompass, color: "#ff9f0a" },
];

export function Sidebar({
  page,
  onChange,
}: {
  page: PageId;
  onChange: (page: PageId) => void;
}) {
  const { connected, bridgeStatus, tapStatus, tapMessage } = useRuntimeStatus();

  const statusRows = [
    {
      key: "remote",
      label: connected ? "遥控器已连接" : "遥控器未连接",
      tone: connected ? "ok" : "off",
    },
    {
      key: "bridge",
      label: bridgeStatus === "running" ? "语音桥运行中" : "语音桥未运行",
      tone: bridgeStatus === "running" ? "ok" : "off",
    },
    {
      key: "tap",
      label: tapStatusLabel(tapStatus),
      tone: tapStatus === "attached" ? "ok" : tapStatus === "pending" ? "warn" : "off",
      title: tapMessage || undefined,
    },
  ];

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
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              className={page === item.id ? "nav-item active" : "nav-item"}
              onClick={() => onChange(item.id)}
            >
              <span className="nav-ico" style={{ background: item.color }}>
                <Icon size={13} />
              </span>
              <span>{item.label}</span>
            </button>
          );
        })}
      </nav>

      <div className="sidebar-status">
        {statusRows.map((row) => (
          <div className="sidebar-status-row" key={row.key} title={row.title}>
            <span className={`status-dot ${row.tone}`} />
            <span>{row.label}</span>
          </div>
        ))}
      </div>
    </aside>
  );
}
