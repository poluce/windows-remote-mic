# AGENTS.md

本文件供 AI Agent / 开发者快速了解 **Remote Mic** 项目。所有 UI 文案使用简体中文。

## 项目是什么

Remote Mic 是一个 Windows 桌面应用，目标是把「小米蓝牙语音遥控器 2 Pro（RC003）」变成电脑的无线麦克风和遥控器：

- 通过蓝牙 BLE 连接 RC003
- 解析 ATVV 协议 / IMA ADPCM 音频
- 支持 Windows 自带语音输入（Win+H）
- 支持按键映射、虚拟声卡（VB-CABLE）输出、诊断自检
- 提供一个右下角扇形快捷菜单（由遥控器「菜单」键呼出）

## 技术栈

- **后端 / 壳**：Rust + Tauri 2
- **前端**：React + TypeScript + Vite
- **核心逻辑**：拆分为多个 workspace crate（`crates/`）
- **目标平台**：Windows

## 目录结构

```
.
├── src/                  # React 前端
│   ├── App.tsx           # 主布局（无顶栏，左侧导航 + 内容区）
│   ├── pages/            # 连接 / 按键映射 / 语音 / 诊断 / 引导
│   └── components/       # Sidebar、Xiaomi2ProRemote 等
├── src-tauri/            # Tauri 壳
│   ├── tauri.conf.json   # 窗口、打包配置
│   └── src/
│       ├── lib.rs        # 启动、窗口创建、共享辅助
│       └── commands/     # 按领域拆分的 Tauri commands
│           ├── connection.rs
│           ├── mapping.rs
│           ├── audio.rs
│           ├── diagnostics.rs
│           ├── log.rs
│           └── quick_menu.rs
├── crates/               # Rust 核心库
│   ├── core-ble          # BLE 扫描 / GATT / 链路
│   ├── core-atvv         # ATVV 协议 + ADPCM 解码
│   ├── core-audio        # WASAPI 端点、DSP、播放、VB-CABLE 诊断
│   ├── core-mapping      # 按键映射、触发条件识别
│   ├── core-config       # 配置持久化（JSON，原子写入）
│   ├── core-voice        # 语音桥 / 模拟链路
│   ├── core-log          # 统一文件日志（分级 + DEBUG 开关）
│   ├── core-stats        # 本机统计
│   ├── core-hid          # Windows Raw Input 框架（可扩展）
│   └── core-diagnostics  # 自检 / 解码预览
├── public/
│   └── quick-menu.html   # 快捷菜单窗口（独立 Canvas 页面）
├── scripts/              # Windows PowerShell 辅助脚本
└── docs/                 # 协议与产品文档（中文）
```

## 常用命令

在 Windows 端执行（从 WSL 可用 `powershell.exe` 调用）：

```powershell
# 开发运行
npm run tauri dev

# 前端构建
npm run build

# Rust 检查（Windows 端）
cargo check -p remote-mic
```

从 WSL 调用示例：

```bash
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command "Set-Location 'F:\B_My_Document\GitHub\windows-remote-mic'; npm run tauri dev"
```

## 关键实现点

### 1. 主窗口与页面
- 主窗口是一个设置型应用，左侧导航，内容区直接渲染页面（已移除顶栏和页面标题）。
- 页面：
  - 连接
  - 按键映射
  - 语音
  - 诊断
  - 引导

### 2. 快捷菜单窗口
- 在 `src-tauri/src/lib.rs` 的 `setup` 中创建第二个透明窗口 `quick-menu`。
- 使用 **工作区（work area）** 定位，避开任务栏；右侧和底部各留 2px。
- 快捷菜单**只有 `public/quick-menu.html` 一份实现**，不要另建 demo 副本。
- 内容为 `public/quick-menu.html`，目前是 **Canvas 2D 实现**：
  - 90° 扇形，双环（外环主菜单、内环工具、中心说明）
  - 物理滚动：弹簧 + 阻尼，VSync 驱动
  - 原版切换/动画逻辑请勿随意改动
  - 边界淡出：外弧内缩 20px，直边内缩 1°
- `toggle_quick_menu` 命令控制显示/隐藏；显示时会 `window.location.reload()` 以便加载最新 HTML。
- 该窗口加载的是静态 HTML，**不走 Vite HMR**；修改后需重新打开窗口（或重启应用）才能生效。
- **热更新坑（重要）**：如果 `toggle_quick_menu` 中“显示时重新加载”的代码被删除：
  ```rust
  let _ = win.eval("window.location.reload()");
  ```
  那么修改 `public/quick-menu.html` 后，快捷菜单窗口不会自动刷新，**必须重启整个应用**才能看到变化。
  所以不要删掉这行；以后若发现“改 HTML 不生效”，优先检查这里是否还在。

### 3. 按键映射持久化
- 后端 `save_mapping` 保存到 `%LOCALAPPDATA%\RemoteMic\RC003\config.json`。
- `get_mappings` 返回完整映射（含 trigger）。
- 前端映射页：
  - 点遥控器图形选按键
  - 选触发方式（单击/双击/长按）
  - 选动作分类/动作
  - 保存后写入后端并更新本地列表
- 映射表只读展示，编辑统一走上方向导。

### 4. ATVV / 音频要点
- GATT 服务：`AB5E0001-...`
  - TX：`...0002`
  - Audio：`...0003`
  - Control：`...0004`
- 重要 opcode：
  - `AUDIO_STOP=0x00`
  - `AUDIO_START=0x04`
  - `MIC_BUTTON=0x08`
  - `AUDIO_SYNC=0x0A`
  - `CAPS=0x0B`
- 音频：IMA/DVI ADPCM 16kHz，高 nibble 优先。
- 语音优先走 Windows 自带语音输入（Win+H）。
- **Win+H 麦克风绑定机制（重要实测结论）**：
  - Windows 11 语音输入（`TextInputHost.exe`）维护专属的持久化音频偏好，**完全无视系统全局默认麦克风的切换**（无论是通过 `IPolicyConfig` 动态改全局默认，还是杀进程冷启动 `TextInputHost.exe` 均无效）。
  - **正确架构与产品规范**：
    - 后端 `AudioSink` 固定将遥控器音频写入 `CABLE Input`；
    - 首次使用时通过引导页/语音页指引用户按 `Win+H`，在语音条设置中**手动将麦克风选择为 `CABLE Output`**（仅需配置一次，Windows 永久记忆）；
    - 这样电脑原本的物理麦克风（如英特尔智音技术）保持为全局默认，开会、微信通话不受任何干扰，遥控器语音输入实现 0 延迟秒级直通。
    - **禁止**在语音链路中做不可靠的“动态改全局默认麦克风”操作。

### 5. 日志
- 统一写入 `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`。
- 每行格式：`[时间] [级别] 内容`，级别包含：
  - `DEBUG` — 临时排错，默认关闭
  - `INFO` — 正常流程
  - `WARN` — 警告
  - `ERROR` — 错误
- 临时开启 DEBUG：
  - 环境变量 `REMOTE_MIC_DEBUG=1`
  - 或创建文件 `%LOCALAPPDATA%\RemoteMic\RC003\debug`（实时生效，无需重启）
- 日志接口统一走 `core-log`：
  - `core_log::log_debug / log_line / log_info / log_warn / log_error`
  - `core-input` 提供 `log_line / log_debug / log_warn / log_error` 薄封装给现有调用方。
- 不要使用 `eprintln!` / `println!` 输出调试信息，统一写入日志文件。
- 筛选示例（PowerShell）：
  ```powershell
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[ERROR\]"
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[DEBUG\]"
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[simulate\]"
  ```

## 约定与注意事项

- **UI 全部使用简体中文。**
- 项目是 clean-room 实现：不要在文档/代码中引用外部仓库（如 HD838A/remote-mic-app）作为来源。
- `scripts/*.ps1` 保持纯 ASCII，避免 PowerShell 编码问题。
- Windows PowerShell 5.1 没有 `Join-String`，需要用 `-join`。
- WSL 与 Windows 共享 node_modules 会产生平台冲突；如遇 esbuild/rollup 平台错误，在 Windows 端重新 `npm install`。
- Vite dev 端口固定为 `1420`；启动前确认端口未被占用。
- Rust 编译请使用 Windows 端 `cargo`；WSL 直接交叉检查 Windows target 可能缺 `llvm-rc`。

## 工作流提示

- 修改前端页面后，运行 `npm run build` 验证。
- 修改 Rust 后，运行 `cargo check -p remote-mic` 验证。
- 提交前检查 `git status`，避免把临时文件或平台相关 node_modules 提交进去。
- 需要重启 Tauri dev 时，先结束旧 `remote-mic.exe` / 占用 `1420` 的进程，再重新启动。