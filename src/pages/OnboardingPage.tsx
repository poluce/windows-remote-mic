import { invoke, isTauri } from "@tauri-apps/api/core";
import { useState } from "react";

const STEPS = [
  {
    title: "1. 蓝牙配对",
    detail: "长按遥控器「主页 + 菜单」键直到指示灯快闪，在 Windows 蓝牙设置中添加并连接 RC003。",
  },
  {
    title: "2. 确认虚拟声卡驱动",
    detail: "确保已安装 VB-CABLE 虚拟音频驱动（未安装可前往「语音」页或官网一键安装）。",
  },
  {
    title: "3. 【关键】绑定 Windows 语音输入麦克风",
    detail:
      "点击下方「唤出语音输入条」或按快捷键 Win+H，在语音条的 ⚙️ 设置中将麦克风选择为「CABLE Output」。（仅需设置一次，Windows 永久记忆；电脑日常物理麦克风保持不变，开会不受干扰，遥控器语音 0 延迟直通）。",
  },
  {
    title: "4. 模拟自检与日常使用",
    detail: "前往「语音」页点击「模拟完整语音链」验证打字上屏；平时长按遥控器语音键即可秒级语音输入。",
  },
];

export function OnboardingPage() {
  const [msg, setMsg] = useState("");

  async function triggerVoiceTyping() {
    if (!isTauri()) {
      setMsg("浏览器预览：请在桌面应用内操作");
      return;
    }
    try {
      const res = await invoke<string>("trigger_voice_typing");
      setMsg(res);
    } catch (err) {
      setMsg(`唤出失败：${err}`);
    }
  }

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
      <section className="card">
        <div className="card-title">快捷入口与配置</div>
        <div className="actions">
          <button className="btn primary" onClick={triggerVoiceTyping}>
            🎙️ 唤出语音输入条（配置麦克风）
          </button>
          <button className="btn" onClick={() => openSetting("bluetooth")}>
            打开 Windows 蓝牙设置
          </button>
          <button className="btn" onClick={() => openSetting("sound")}>
            打开声音设置
          </button>
          <button className="btn" onClick={() => openSetting("microphone")}>
            打开麦克风隐私设置
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
