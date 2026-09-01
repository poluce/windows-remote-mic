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
│   ├── pages/            # 连接 / 按键映射 / 诊断 / 引导
│   ├── store/            # 运行时状态（连接 / 语音桥 / 旁路）
│   └── components/       # Sidebar、Xiaomi2ProRemote、RemoteKeyTester
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
│   ├── core-dispatch     # 按键调度：触发检测 → 映射 → 动作执行
│   ├── core-config       # 配置持久化（JSON，原子写入）
│   ├── core-voice        # 语音桥 / 模拟链路
│   ├── core-log          # 统一文件日志（分级 + DEBUG 开关）
│   ├── core-stats        # 本机统计
│   ├── core-hid          # HID 底层事件捕获 / 报告解析（Raw Input）
│   └── core-input        # Windows 按键注入、热键、输入钩子
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

### 3.5 按键来源分类（三类流）
- 当前物理按键来源分三类：
  | 类别 | 链路 | 按键（usage） |
  | --- | --- | --- |
  | ATVV 控制流（非 HID） | BLE `Control` -> `core-voice` | 麦克风（主路径；HID 兜底 `0x3E`） |
  | HID 标准流 | 标准 HID 键盘报告 -> Raw Input / WH_KEYBOARD_LL | 上 `0x52`、下 `0x51`、左 `0x50`、右 `0x4F`、OK `0x28`、主页 `0x4A`、菜单 `0x65`、电源 `0x66` |
  | HID 应用命令类 / HOGP 旁路 | HidOverGatt IOCTL / Frida Tap | 返回 `0xF1`、音量+ `0x80`、音量− `0x81`、TV `0x35` |
- 全部 13 键 usage 表（HID 键盘页 `0x07`）：
  | 物理按键 | usage | 标准性 | Windows vkey |
  | --- | --- | --- | --- |
  | 麦克风（F5 兜底） | `0x3E` | 标准 | 116 |
  | 返回 | `0xF1` | 厂商自定义（0xE8–0xFF 保留区） | 166 |
  | 确定 | `0x28` | 标准 | 13 |
  | TV | `0x35` | 标准（Launch Media Select） | 180 |
  | 主页 | `0x4A` | 标准 | 36（实测，不是 172） |
  | 右 | `0x4F` | 标准 | 39 |
  | 左 | `0x50` | 标准 | 37 |
  | 下 | `0x51` | 标准 | 40 |
  | 上 | `0x52` | 标准 | 38 |
  | 菜单 | `0x65` | 标准 | 93 |
  | 电源 | `0x66` | 标准 | 255 |
  | 音量+ | `0x80` | 标准（Keyboard Volume Up） | 175 |
  | 音量− | `0x81` | 标准（Keyboard Volume Down） | 174 |
  - 注：静音 usage `0x7F` 是标准键盘页 usage，但**遥控器无实体静音键**，项目映射表（`usage_to_vkey`）刻意不包含它，不参与任何按键流。
- 三类流最终汇聚方式：
  - 麦克风键双路径，状态在 `core-input` 统一：ATVV Control 的 `MicButtonPressed` 由 `core-voice` bridge 调 `core_input::toggle_voice_typing`；HID 兜底 `0x3E` → vkey 116 进调度器，执行默认映射 Mic → Voice（同样调 `toggle_voice_typing`）。真机实测 ATVV Control 通道收不到控制包，麦克风键实际走 HID 兜底。
  - 其余 12 键：Raw Input / HOGP 旁路 → `core_dispatch::KeyDispatcher`（每键一个 `TriggerDetector`）→ 查 `MappingConfig` → `core-input` 注入，并写入 `core-stats`。
  - 映射页保存后通过 `save_mapping` 热更新调度器；诊断页按键测试进行时调用 `set_dispatch_enabled(false)` 暂停调度。
- 音频流与按键流是不同线程：音频走 GATT Audio 回调 -> channel -> 桥接主线程 -> `AudioSink`；按键/控制走 GATT Control 回调或 HID Hook / Raw Input 线程。

#### 3.5.1 返回 / 音量 / TV 键的信号通道（重要实测结论）
- **usage 标准性**：
  - 音量+ `0x80`、音量− `0x81` 是 **HID 键盘页标准 usage**（Keyboard Volume Up/Down）。
  - TV `0x35` 是 **HID 键盘页标准 usage**（Keyboard Launch Media Select）。
  - 返回 `0xF1` 是**厂商自定义 usage**（HID 键盘页 0xE8–0xFF 为保留区，小米自行使用）。
- **四个键在 Windows 里的命运完全相同**（都属于「应用命令类按键」）：
  - 系统**认识并原生处理**它们：返回 → `VK_BROWSER_BACK`（浏览器后退）、音量 → `VK_VOLUME_UP/DOWN`（系统音量变化）、TV → `VK_LAUNCH_MEDIA_SELECT`，走 WM_APPCOMMAND 应用命令通道（推断，待真机验证）。
  - 它们被 HOGP 层从「键盘输入通道」分流，所以 **Raw Input 和 WH_KEYBOARD_LL 钩子都看不到**（实测）。
  - 但 **Frida 旁路（HOGP 驱动层）四个键都能捕获到**（实测，日志可见 `0x00F1` / `0x0080` / `0x0081` / `0x0035`）。
  - 返回键**没有被系统丢弃**——它和音量/TV 键完全同类，只是 usage 编号是厂商自定义的。
- **主页键 vkey 映射（已修复）**：键盘页 `0x4A` 实测 Windows 映射为 `VK_HOME(36)`，不是 `VK_BROWSER_HOME(172)`。`usage_to_vkey(0x4A)` 已改为 36；消费类页 `0x0223` 仍映射 172（不同通道）。
- **双重处理问题**：系统原生执行（音量+1 / 浏览器后退）+ 旁路注入映射动作 = 一个信号做两件事。普通键（方向/OK/菜单等）同样存在此问题（系统原生 + Raw Input 注入）。现已由驱动层「吃掉」模式解决（见下）。
- **「吃掉」模式（已实现并真机验证，默认开启，连接页可切换）**：真实报告路径是 **GATT 通知 → 8 字节队列项 → `0x1febc` 分配 WDF 请求缓冲区 → `0x20080` 的 memcpy 拷入 → 完成请求 → 内核 HID 驱动**。Frida 脚本钩住 `0x20080` 这个 memcpy 调用点：先把清零前的队列项原始报告转成 9 字节 IOCTL 格式（`01 00 00` + 3 个 usage）以 `gatt_read` 上报应用调度器，再把源缓冲区清零——系统 HOGP 层看不到任何遥控器按键，只由本应用注入映射动作。
  - **持久化与热切换**：开关存于 `config.json` 的 `hid_tap_eat`（默认 `true`）；后端启动/切换时写入 `%PROGRAMDATA%\RemoteMic\hid-tap\eat-mode.txt`，Frida 脚本每秒轮询该文件热更新，**切换无需重新注入 WUDFHost / 不弹 UAC**。注入时 `frida-gadget.config` 的 `parameters.eat` 只是初始值。
  - **优先级**：`eat-mode.txt`（用户设置）> 环境变量 `REMOTE_MIC_HID_TAP_EAT`（开发兜底）> 默认开启。
  - 注意：eat 开启时如果某键未配映射动作，该键对系统与本应用都无效（信号被应用独家持有）。
  - **旧方案（清 READ_CHARACTERISTIC_IOCTL 输出缓冲）已证明无效并已删除**：该 IOCTL 是报告处理完后的「补读」，输出缓冲区实测返回全零，不是系统消费的数据。
  - 驱动更新导致偏移失效时，按 skill `hogp-report-path-re` 重新定位（见 `.agent/skills/hogp-report-path-re/SKILL.md`）。
- **已废弃的规划**：曾计划在 WH_KEYBOARD_LL 钩子里拦截普通键（return 1）实现「单一持有者」。驱动层 eat 生效后不再需要；且钩子层面无法区分遥控器与 PC 键盘（vkey+scan code 相同，实测会把 PC 键盘的静音键 173 也当遥控器键转发）。
- **HOGP 探针代码已收敛**：RE 期的诊断噪音（模块/导入枚举、函数表 dump、IOCTL 全量追踪、反汇编/调用栈上报）已删除。保留的轻量追踪（`report_path` / `memcpy_trace` / `vcall_trace`）仅在 `REMOTE_MIC_HID_TAP_TRACE=1` 时输出，用于偏移失效时重新定位。

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
  - 创建文件 `%LOCALAPPDATA%\RemoteMic\RC003\debug`（实时生效，无需重启）
  - 诊断页「开启 DEBUG」按钮（调用 `core_log::set_debug_enabled`，运行时生效）
- 日志轮转（`core-log` 自动处理）：
  - 主日志超过 `MAX_LOG_BYTES`（默认 2 MiB）时自动改名为 `remote-mic.<时间戳>.log`
  - 轮转备份保留 `KEEP_BACKUP_FILES`（默认 5 份），更旧的自动删除
- 日志接口统一走 `core-log`：
  - `core_log::log_debug / log_line / log_info / log_warn / log_error`
  - `core-log` 还提供 `read_log_tail / clear_log / log_files / debug_enabled / set_debug_enabled` 供诊断页与 Tauri 命令使用
  - `core-input` 提供 `log_line / log_debug / log_warn / log_error` 薄封装给现有调用方。
- 不要使用 `eprintln!` / `println!` 输出调试信息，统一写入日志文件。
- 诊断页可查看日志尾部、清空日志、打开日志目录、切换 DEBUG。
- 筛选示例（PowerShell）：
  ```powershell
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[ERROR\]"
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[DEBUG\]"
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[simulate\]"
  ```

## 约定与注意事项

- **UI 全部使用简体中文。**
- **crate 边界**：
  - `core-hid` 只负责 HID 底层事件捕获 / 报告解析（Raw Input、HID 报告、HOGP 旁路）。
  - `core-input` 只负责 Windows 输入注入、热键、输入钩子 / 动作执行。
  - `core-mapping` 只负责按键 / 触发 / 动作的纯逻辑定义。
  - `core-dispatch` 负责把物理按键接到映射表并执行动作；不要把 SendInput 或 Raw Input 细节塞回 mapping。
  - 日志统一以 `core-log` 为唯一出口；`core-input` 现有的 `log_*` 薄封装仅为兼容旧调用，新代码不要再增加这类包装。
- `scripts/*.ps1` 保持纯 ASCII，避免 PowerShell 编码问题。
- Windows PowerShell 5.1 没有 `Join-String`，需要用 `-join`。
- WSL 与 Windows 共享 node_modules 会产生平台冲突；如遇 esbuild/rollup 平台错误，在 Windows 端重新 `npm install`。
- Vite dev 端口固定为 `1420`；启动前确认端口未被占用。
- Rust 编译请使用 Windows 端 `cargo`；WSL 直接交叉检查 Windows target 可能缺 `llvm-rc`。

## 分支 / PR / 提交规范

- 分支策略：
  - `main` 为受保护主干，始终应保持可构建、可运行。
  - 功能/修复请从 `main` 切分支，命名如 `feat/xxx`、`fix/xxx`、`docs/xxx`。
  - 完成并通过本地验证后，通过 Pull Request 合入 `main`。
- PR 要求：
  - 标题简明，说明改动内容。
  - 描述关联的 issue / 任务项（如有）。
  - 必须通过 CI：前端 typecheck/build、Rust fmt/clippy/test、Tauri check。
  - 涉及 UI 改动时附上简短的改动说明或截图（可选）。
- 提交信息规范（Conventional Commits 风格）：
  - `feat: 新功能`
  - `fix: 修复问题`
  - `docs: 文档变更`
  - `chore: 构建/工具/依赖等杂项`
  - `refactor: 重构`
  - `test: 测试相关`
- 发版流程见 `CHANGELOG.md`：更新版本号 → 更新 CHANGELOG → 打 tag → CI 自动出 Release。

## 工作流提示

- 修改前端页面后，至少运行 `npm run typecheck`，完整验证用 `npm run build`。
- 修改 Rust 后，运行以下命令验证：
  ```powershell
  cargo fmt --all -- --check
  cargo clippy --workspace --exclude remote-mic --all-targets -- -D warnings
  cargo check -p remote-mic
  ```
- 提交前检查 `git status`，避免把临时文件或平台相关 node_modules 提交进去。
- 需要重启 Tauri dev 时，先结束旧 `remote-mic.exe` / 占用 `1420` 的进程，再重新启动。
- **HOGP 旁路 / 吃掉模式失效时**：先调用 skill `hogp-report-path-re`（`.agent/skills/hogp-report-path-re/SKILL.md`），按其中的方法论与 Playbook 重新定位驱动报告写入点；不要凭旧偏移盲改。