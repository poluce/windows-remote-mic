import { useEffect, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

type MappingEntry = {
  button: string;
  name: string;
  action: string;
};

const FALLBACK_MAPPING: MappingEntry[] = [
  { button: "up", name: "上", action: "↑" },
  { button: "down", name: "下", action: "↓" },
  { button: "left", name: "左", action: "←" },
  { button: "right", name: "右", action: "→" },
  { button: "ok", name: "确定", action: "回车（Enter）" },
  { button: "back", name: "返回", action: "删除（退格）" },
  { button: "home", name: "主页", action: "显示桌面（Win+D）" },
  { button: "menu", name: "菜单", action: "右键菜单（上下文菜单）" },
  { button: "tv", name: "TV", action: "切换应用（Alt+Tab）" },
  { button: "power", name: "电源", action: "取消（Esc）" },
  { button: "volume_up", name: "音量 +", action: "音量 +" },
  { button: "volume_down", name: "音量 −", action: "音量 −" },
  { button: "mic", name: "麦克风", action: "语音输入（Win+H）" },
];

export function MappingPage() {
  const [mapping, setMapping] = useState<MappingEntry[]>(FALLBACK_MAPPING);

  useEffect(() => {
    if (!isTauri()) return;
    invoke<MappingEntry[]>("default_mapping")
      .then((list) => setMapping(list.length ? list : FALLBACK_MAPPING))
      .catch(() => {});
  }, []);

  return (
    <div className="page">
      <h2>按键映射</h2>
      <p className="page-sub">13 个按键（12 个普通键 + 1 个麦克风键）的当前动作。</p>

      <section className="card">
        <div className="card-title">默认映射（来自 Rust 后端）</div>
        <div className="mapping-list">
          {mapping.map((b) => (
            <div key={b.button} className="mapping-row">
              <span className="mapping-key">{b.name}</span>
              <span className="mapping-action">{b.action}</span>
              <button className="btn small" disabled>
                编辑
              </button>
            </div>
          ))}
        </div>
        {!isTauri() && (
          <p className="hint">浏览器预览：显示内置默认值；桌面应用内从 Rust 后端读取。</p>
        )}
      </section>
    </div>
  );
}
