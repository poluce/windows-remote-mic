import {
  tapStatusLabel,
  useEffectiveConnected,
  useRuntimeStatus,
} from "../store/runtimeStatus";
import { useUpdateStatus } from "../store/updateStatus";
import {
  IconActivity,
  IconBluetooth,
  IconCompass,
  IconGrid,
  IconLog,
  IconSliders,
} from "./icons";

export type PageId =
  | "connection"
  | "mapping"
  | "settings"
  | "diagnostics"
  | "log"
  | "guidance";

// 导航只用单色线性图标：颜色不再承担「区分页面」的职责，
// 选中态靠白玻璃胶囊 + 左侧竖条 + 强调色图标表达（见 App.css 的 .nav-item.active）。
const NAV_ITEMS: {
  id: PageId;
  label: string;
  icon: typeof IconBluetooth;
}[] = [
  { id: "connection", label: "连接", icon: IconBluetooth },
  { id: "mapping", label: "按键映射", icon: IconGrid },
  { id: "settings", label: "通用设置", icon: IconSliders },
  { id: "diagnostics", label: "诊断", icon: IconActivity },
  { id: "log", label: "日志", icon: IconLog },
  { id: "guidance", label: "引导", icon: IconCompass },
];

export function Sidebar({
  page,
  onChange,
}: {
  page: PageId;
  onChange: (page: PageId) => void;
}) {
  const { bridgeState, tapStatus, tapMessage } = useRuntimeStatus();
  const { phase: updatePhase } = useUpdateStatus();
  // 派生视图：等待遥控器上线时绝不能显示「遥控器已连接」。
  const connected = useEffectiveConnected();
  const bridgeRunning = bridgeState === "running";
  const bridgeWaiting = bridgeState === "waiting";

  const statusRows = [
    {
      key: "remote",
      label: connected ? "遥控器已连接" : "遥控器未连接",
      tone: connected ? "ok" : "off",
    },
    {
      key: "bridge",
      label: bridgeRunning
        ? "语音桥运行中"
        : bridgeWaiting
          ? "等待遥控器上线"
          : "语音桥未运行",
      tone: bridgeRunning ? "ok" : bridgeWaiting ? "warn" : "off",
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
              <span className="nav-ico">
                <Icon size={16} />
              </span>
              <span>{item.label}</span>
              {item.id === "settings" && updatePhase === "available" && (
                <span
                  className="nav-badge"
                  title="有新版本可用，点击查看"
                  aria-label="有新版本可用"
                />
              )}
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
