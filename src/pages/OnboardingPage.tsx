import { invoke, isTauri } from "@tauri-apps/api/core";
import { useState } from "react";

const STEPS = [
  { title: "进入配对", detail: "长按遥控器「主页 + 菜单」键，直到指示灯进入配对状态。" },
  { title: "Windows 蓝牙配对", detail: "打开 设置 → 蓝牙和其他设备，连接 MI RC / Xiaomi Bluetooth Remote 2 Pro。" },
  { title: "蓝牙权限", detail: "确认系统蓝牙已开启；应用会在连接时请求访问权限。" },
  { title: "选择输出设备", detail: "在「语音」页把输出端点选为 CABLE Input（VB-CABLE）。" },
  { title: "安装虚拟声卡", detail: "未安装 VB-CABLE 时，从 https://vb-audio.com/Cable/ 下载安装；语音输入法的麦克风选 CABLE Output。" },
  { title: "语音输入方式", detail: "优先使用 Windows 自带语音输入（Win+H），无需第三方输入法。" },
  { title: "运行检查", detail: "到「诊断」页运行音频检查 + 测试音，确认 CABLE 链路正常。" },
];

export function OnboardingPage() {
  const [msg, setMsg] = useState("");

  async function openSetting(kind: string) {
    if (!isTauri()) {
      setMsg("浏览器预览：请在桌面应用内打开系统设置");
      return;
    }
    try {
      setMsg(await invoke<string>("open_system_settings", { setting: kind }));
    } catch (err) {
      setMsg(`打开失败：${err}`);
    }
  }

  return (
    <div className="page">
      
      <p className="page-sub">第一次使用：按顺序完成以下步骤。</p>

      <section className="card">
        <div className="card-title">快捷入口</div>
        <div className="actions">
          <button className="btn" onClick={() => openSetting("bluetooth")}>
            打开 Windows 蓝牙设置
          </button>
          <button className="btn" onClick={() => openSetting("microphone")}>
            打开麦克风隐私设置
          </button>
          <button className="btn" onClick={() => openSetting("sound")}>
            打开声音设置
          </button>
        </div>
        {msg && <p className="hint">{msg}</p>}
      </section>

      <section className="card">
        <div className="card-title">设置向导</div>
        <ol className="onboarding-list">
          {STEPS.map((s, i) => (
            <li key={s.title}>
              <span className="step-no">{i + 1}</span>
              <div>
                <div className="step-title">{s.title}</div>
                <div className="step-detail">{s.detail}</div>
              </div>
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}
