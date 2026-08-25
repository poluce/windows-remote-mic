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

## 项目结构

```text
remote-mic/
├── src/                     # 前端（TypeScript + React）
├── src-tauri/               # Tauri 桌面壳（Rust）
├── crates/
│   ├── core-ble/            # 蓝牙 BLE/GATT
│   ├── core-atvv/           # ATVV 协议 + ADPCM 解码
│   ├── core-audio/          # 音频输出（WASAPI）
│   ├── core-hid/            # HID / Raw Input
│   ├── core-input/          # 按键注入 / 动作执行
│   ├── core-mapping/        # 按键映射
│   ├── core-config/         # 配置持久化
│   └── core-diagnostics/    # 诊断
└── docs/                    # 规划与任务文档
```

## 开发

```bash
# 安装前端依赖
npm install

# 启动前端（仅浏览器预览）
npm run dev

# 启动 Tauri 桌面应用
npm run tauri dev

# 构建发布包
npm run tauri build
```

## 文档

- [规划文档](docs/规划.md)
- [ToDo / 任务清单](docs/任务清单.md)
- [ATVV 协议事实表](docs/ATVV协议.md)
- [社区参考资料](docs/社区资料/来源清单.md)