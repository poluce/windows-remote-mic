# Windows 无线麦（Remote Mic）

把小米蓝牙语音遥控器变成 Windows 的无线麦克风。

支持：

- 蓝牙连接小米遥控器（RC003 / 后续扩展 RC001）
- 语音输入：优先使用 Windows 自带语音输入（Win+H）
- 普通按键映射：方向、确定、返回、主页、菜单、TV、电源、音量等
- 虚拟声卡路由：输出到 VB-CABLE 等虚拟音频设备

## 技术栈

- 核心后端：Rust
- 桌面界面：Tauri 2 + TypeScript / React
- 蓝牙：WinRT BLE
- 音频：WASAPI

## 文档

- [规划文档](docs/PLANNING.md)
- [ToDo / 任务清单](docs/TODO.md)

## 状态

早期规划阶段。