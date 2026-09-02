# Windows 无线麦（Remote Mic）

把小米蓝牙语音遥控器变成 Windows 的无线麦克风。

支持：

- 蓝牙连接小米遥控器（RC003 / 后续扩展 RC001）
- 语音输入：优先使用 Windows 自带语音输入（Win+H）
- 按键映射：13 键全部走映射表，支持单击 / 双击 / 长按（长按只触发一次）
- 麦克风键：长按门控的「按下 / 松开」触发（默认 Win+H，可改为任意动作，支持第三方语音助手）
- 驱动层「拦截 HID 按键信号」模式（默认开启）：在 WUDFHost HOGP 驱动写入点吃掉原始报告，由本应用独家按键，避免系统原生动作与应用映射动作双重触发；连接页可实时切换
- 触发时间可配置：长按阈值（默认 550ms）、双击窗口（默认 300ms），映射页可调
- 快捷菜单：右下角扇形菜单（默认由遥控器「菜单」键呼出，可改映射），支持遥控器独占操作与状态记忆
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
│   ├── core-ble/            # 蓝牙 BLE/GATT 连接
│   ├── core-atvv/           # ATVV 协议 + ADPCM 解码
│   ├── core-audio/          # WASAPI 音频输出、DSP、诊断
│   ├── core-hid/            # HID 事件捕获 / 报告解析（Raw Input + HOGP 旁路）
│   ├── core-input/          # Windows 按键注入、热键、输入钩子
│   ├── core-mapping/        # 按键映射与触发规则
│   ├── core-dispatch/       # 按键调度（触发 → 映射 → 执行）
│   ├── core-config/         # 配置持久化
│   ├── core-voice/          # 语音桥：BLE -> 解码 -> 输出
│   ├── core-log/            # 统一文件日志
│   └── core-stats/          # 本机统计
├── public/
│   └── quick-menu.html      # 快捷菜单独立页面（遥控器独占操作）
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

## 质量检查

```bash
# 前端类型检查（CI 会执行）
npm run typecheck

# Rust 格式检查（CI 会执行）
cargo fmt --all -- --check

# 核心 crates Clippy（CI 会执行，warning 视为错误）
cargo clippy --workspace --exclude remote-mic --all-targets -- -D warnings

# Rust 测试
cargo test --workspace --exclude remote-mic
```

## 文档

- [规划文档](docs/项目/规划.md)
- [ToDo / 任务清单](docs/项目/任务清单.md)
- [发布说明](docs/项目/发布说明.md)
- [真机验收](docs/项目/真机验收.md)
- [问题记录：读取返回/音量键](docs/项目/问题记录-读取返回音量键.md)
- [CHANGELOG / 发版流程](CHANGELOG.md)
- [RC003 开发探索信息总档](docs/协议/ATVV/RC003-开发探索信息总档.md)
- [ATVV 协议事实表](docs/协议/ATVV/协议.md)