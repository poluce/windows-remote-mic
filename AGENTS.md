# AGENTS.md

本文件供 AI Agent / 开发者快速了解 **Remote Mic** 项目。所有 UI 文案使用简体中文。

## 项目是什么

Remote Mic 是一个 Windows 桌面应用，目标是把「小米蓝牙语音遥控器 2 Pro（RC003）」变成电脑的无线麦克风和遥控器：

- 通过蓝牙 BLE 连接 RC003（ATVV GATT 服务），解析 IMA ADPCM 音频
- 语音写入虚拟声卡 VB-CABLE（`CABLE Input`），供 Windows 语音输入（Win+H）或第三方输入法使用
- 13 个按键全部可映射（单击 / 双击 / 长按 / 按下 / 松开），支持应用档案（按前台进程切换）、自定义快捷键、按键校准
- HOGP 旁路「吃掉」按键，避免系统原生动作与映射动作重复触发
- 右下角扇形快捷菜单（默认由遥控器「菜单」键呼出，可改映射）
- 诊断自检、日志页、首次引导（4 步）

> 协议 / 按键 / 音频 / 系统行为的实测知识沉淀见 [docs/协议/RC003-开发探索信息总档.md](docs/协议/RC003-开发探索信息总档.md)。
> 容错改进轮次与状态一致性审计见 [docs/项目/容错能力评估.md](docs/项目/容错能力评估.md)；
> 待办清单见 [docs/项目/任务清单.md](docs/项目/任务清单.md)；真机验收项见 [docs/项目/真机验收.md](docs/项目/真机验收.md)。

## 当前状态（v0.7.3）

- `main` 版本 **0.7.3**；已发 tag：`v0.6.0`、`v0.6.1`、`v0.6.2`、`v0.7.0`、`v0.7.1`、`v0.7.2`、`v0.7.3`。
- 发布产物：NSIS 安装包 `Remote.Mic_<ver>_x64-setup.exe`、免安装 `remote-mic.exe`、`SHA256SUMS.txt`，
  以及自动更新用的 `Remote.Mic_<ver>_x64-setup.exe.sig`、`latest.json`（见 [docs/项目/自动更新.md](docs/项目/自动更新.md)）。
- 自动更新：应用内「通用设置 → 关于与更新」可检查 / 下载 / 安装新版本（Tauri v2 updater，NSIS passive 安装，需 UAC）；
  正式构建启动后立即静默检查一次，运行期间每 30 分钟复查；自动检查距上次成功检查 ≥1 小时才真正联网，
  手动检查不受频率限制（时间戳持久化在 `config.json` 的 `last_update_check_at`）。
- 安装包**未做代码签名**，首次运行 SmartScreen 提示「未知发布者」属预期；虚拟 HID 驱动为测试签名，安装器会把随包证书装入本机信任库。
- 已知未解决问题：**ATVV 通知偶发丢失**（HID 正常、收不到语音包），见 §10。

## 技术栈

- **后端 / 壳**：Rust + Tauri 2（Windows x64）
- **前端**：React 19 + TypeScript + Vite（WebView2）
- **核心逻辑**：拆分为多个 workspace crate（`crates/`）
- **辅助脚本**：PowerShell（驱动打包 / VB-CABLE / HOGP 旁路）
- **CI**：GitHub Actions（`windows-latest`），发版由 tag 触发

## 目录结构

```
.
├── src/                      # React 前端
│   ├── App.tsx               # 主布局（无顶栏，左侧导航 + 内容区）
│   ├── pages/                # 连接 / 按键映射 / 通用设置 / 诊断 / 日志 / 引导
│   │   ├── connection/       # 连接页 hooks（设备动作 / 虚拟声卡下拉）
│   │   └── mapping/          # 映射页子组件（矩阵、作用域、快捷键、校准…）
│   ├── store/runtimeStatus.ts# 运行时状态单一事实来源（连接 / 语音桥 / 旁路）
│   ├── store/updateStatus.ts # 更新状态单一事实来源（检查 / 下载进度 / 新版本）
│   ├── components/           # Sidebar、Xiaomi2ProRemote、RemoteKeyTester、ErrorBoundary…
│   └── constants.ts          # OUTPUT_DEVICE = "CABLE Input"
├── src-tauri/                # Tauri 壳
│   ├── tauri.conf.json       # 窗口/打包；beforeBuildCommand = npm run build:installer；NSIS perMachine
│   ├── windows/
│   │   ├── hooks.nsh         # 安装/卸载虚拟 HID 驱动、复制 VB-CABLE 助手脚本
│   │   ├── driver/           # prepare-vhid-bundle.ps1 生成（gitignored，勿提交）
│   │   └── vb-cable/         # prepare-installer-extras.ps1 生成（gitignored，勿提交）
│   └── src/
│       ├── lib.rs            # 应用装配：命令注册、窗口创建、调度器装配、生命周期
│       ├── support/          # 外壳通用支撑：路径 / 状态 / 按键兜底 / panic / 映射编解码
│       └── commands/         # connection / mapping / mapping_catalog / voice_bridge /
│                             # audio_tools / vb_cable / app_profile（分 menu/editor） / diagnostics / log / quick_menu
├── crates/                   # Rust 核心库
│   ├── core-ble              # BLE 扫描 / GATT / ATVV 服务 / LinkWatcher / wait_op（分 gatt 与 watcher）
│   ├── core-atvv             # ATVV 协议 + ADPCM 解码 + CAPS 帧长协商
│   ├── core-audio            # WASAPI 端点、格式协商/重采样、播放、VB-CABLE 诊断
│   ├── core-mapping          # 映射 / 触发 / 动作 / VoiceTarget 纯逻辑
│   ├── core-dispatch         # 按键调度、应用档案匹配、运行时事件、独占输入模式
│   ├── core-config           # 配置 JSON 原子写入 + .bak 回退
│   ├── core-voice            # 语音桥：ports（端口 trait）/ transport / sink / keys /
│   │                         #   bridge（会话编排）/ supervisor（重连状态机）/ simulate
│   ├── core-log              # 统一日志（分级 / 轮转 / DEBUG 开关）
│   ├── core-stats            # 本机统计（批量写盘）
│   ├── core-hid              # HID 捕获（Raw Input + HOGP 旁路 Frida；tap 模块化为 config/hub/inject/runtime/status/watchdog）
│   ├── core-input            # 按键注入、Win+H、单实例互斥等 Windows 输入能力
│   └── core-app-profile      # 前台应用档案（按进程 exe 匹配、种子模板）
├── profiles/                 # 应用档案种子模板（*.json，首次启动播种到用户目录）
├── public/quick-menu.html    # 快捷菜单窗口（独立 Canvas 页面）
├── scripts/                  # PowerShell 辅助脚本（构建 / 驱动 / VB-CABLE / Tap）
├── .agent/skills/            # HOGP 旁路逆向 skill（hogp-report-path-re）
├── .github/workflows/        # ci.yml / release.yml
├── docs/                     # 协议、项目、设计文档（中文）
└── CHANGELOG.md
```

## 常用命令

### 开发与构建（Windows 端）

```powershell
npm install
npm run tauri dev          # 开发运行（Vite 端口 1420，前端热更新）
npm run typecheck          # tsc --noEmit
npm run build              # 前端构建（tsc + vite build）
npm run build:installer    # 前端构建 + stage VB-CABLE 助手脚本（tauri build 会自动调用）
npm run tauri build        # 打 NSIS 安装包
```

> 本地打**完整**安装包前，先运行 `powershell -ExecutionPolicy Bypass -File scripts/prepare-vhid-bundle.ps1`
> 把虚拟 HID 驱动 bundle 放进 `src-tauri/windows/driver/`；`hooks.nsh` 对缺失 bundle 用 `File /nonfatal`，所以
> 不准备也能编译，但装出来的包不含驱动。

### 本地验证（提交 PR 前跑全量）

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --exclude remote-mic --all-targets -- -D warnings
cargo test --workspace --exclude remote-mic
cargo check -p remote-mic
cargo test -p remote-mic          # app crate 单测；CI 只做 check，建议本地跑
npm run typecheck
npm run build
```

v0.7.0 基线（含本次重构）：workspace 测试 **183** 个、app crate 测试 **5** 个。
（原 `commands/audio.rs` 的 3 个测试已随监督状态机迁到 `core-voice/supervisor.rs`。）

### 从 WSL 调用（路径按本机实际情况替换）

```bash
/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -Command \
  "Set-Location 'D:\path\to\windows-remote-mic'; npm run tauri dev"
```

## 关键实现点

### 1. 主窗口与页面

- 主窗口是设置型应用：左侧导航，内容区直接渲染页面（无顶栏、无页面标题）。
- 页面（实际导航，见 `src/components/Sidebar.tsx`）：
  - **连接**：设备卡（扫描 / 连接 / 断开）、连接状态 pills（语音桥 / 返回音量旁路 / ATVV 端点）、
    语音链路（虚拟声卡选择、语音桥状态）、等待 / 重试 / 失败提示。
  - **按键映射**：遥控器按键矩阵、触发方式、动作选择、应用档案作用域、自定义快捷键、按键校准。
  - **通用设置**：拦截 HID 按键信号（eat 模式）、长按判定、双击间隔。
  - **诊断**：状态与自检（10 项）、VB-CABLE 检测 / 一键安装、应用专属映射概览、按键测试、测试音。
  - **日志**：查看尾部、清空、打开目录、DEBUG 开关。
  - **引导**：4 步首次配置（蓝牙配对 → 虚拟声卡 → Win+H 麦克风绑定 → 模拟自检与日常使用）。
- **没有独立的「语音」页和「统计」页**：语音相关在连接页，统计只在后台落盘。

### 2. 快捷菜单窗口

- 在 `src-tauri/src/lib.rs` 的 `setup` 中创建第二个透明窗口 `quick-menu`。
- 使用**工作区（work area）**定位，避开任务栏；右侧和底部各留 2px。
- 快捷菜单**只有 `public/quick-menu.html` 一份实现**，不要另建 demo 副本。
- 内容为 Canvas 2D 实现：
  - 90° 扇形，双环（外环主菜单、内环工具、中心说明）
  - 物理滚动：弹簧 + 阻尼，VSync 驱动
  - 原版切换 / 动画逻辑请勿随意改动
  - 边界淡出：外弧内缩 20px，直边内缩 1°
- `toggle_quick_menu` / `hide_quick_menu` 控制显示 / 隐藏；显示时会 `window.location.reload()` 以便加载最新 HTML。
- **菜单键默认动作是 `ToggleQuickMenu`**，旧配置 `ContextMenu`（右键菜单）会在启动时自动迁移。
- **菜单独占输入模式**：打开快捷菜单时调度器进入 `InputMode::QuickMenu`，遥控器方向 / 确定 / 菜单 / 返回等按键
  不再走普通触发检测，而是以 `quick-menu-key` 事件转发给快捷菜单窗口；关闭后恢复 `InputMode::Normal`。
- **状态持久化**：`quick-menu.html` 把菜单停留位置与所选环状态存入 `localStorage`（key `remote-mic-quick-menu-state`）。
- 该窗口加载静态 HTML，**不走 Vite HMR**；修改后需重新打开窗口（或重启应用）才能生效。
- **热更新坑**：不要删掉 `toggle_quick_menu` 里显示时的 `window.location.reload()`，
  否则改 `quick-menu.html` 后必须重启整个应用才能看到变化。

### 3. 运行时状态与单一事实来源（重要约定）

- 前端运行时状态**只从 `src/store/runtimeStatus.ts` 取**，不要在各页面各存一份：
  - `connected`（后端事实）、`tapStatus`、`endpointsReady`、`bridgeState / bridgeAttempt /
    bridgeFailures / bridgeNextRetrySecs / bridgeLastError`。
  - `initRuntimeStatus()` 启动时拉 `get_runtime_status`，并订阅后端事件
    `ble-connection-status`、`bridge-status`、`hid-tap-status`。
- `useEffectiveConnected() = connected && bridgeState !== "waiting"`：
  避免「已连接 + 等待遥控器上线」这种矛盾展示。新增 UI 请使用该派生值，不要自己拼状态。
- **ATVV 端点就绪**只有一个事实来源：后端 `BridgeStatus.endpoints_ready`（随每次快照同步）。
  后端 `mark_session_started` 先置位端点就绪、再触发 running 回调；有回归测试
  `endpoints_ready_is_set_before_running_callback`。前端不要再维护第二份。
- `scan_for_rc003` 可能命中 WinRT GATT 缓存，即使链路断开也能发现端点；
  **判断是否已连接必须看 `Rc003Connection.link_connected`**，不能凭「发现成功」推断。
- 其他单一事实来源：
  - `OUTPUT_DEVICE`（`src/constants.ts`）= 语音输出设备名；
  - `get_mapping_catalog`（`src-tauri/src/commands/mapping_catalog.rs`）= 按键 / 触发 / 动作目录；
    前端 `src/pages/mapping/constants.ts` 的 `FALLBACK_CATALOG` 仅供浏览器预览兜底；
  - `src/store/updateStatus.ts` = 更新检查 / 下载进度 / 新版本号的唯一状态源（设置页与侧栏提示共用）。
- 历史坑：前端曾保留 `bridgeStatus` 副本、后端曾保留本地 stop 标志，都因状态不同步出过 bug，**不要加回来**。

### 4. 按键映射、应用档案与快捷键

- 配置持久化到 `%LOCALAPPDATA%\RemoteMic\RC003\config.json`：
  - 原子写入；损坏时自动回退 `config.json.bak`（上一次成功保存）。
  - 关键字段：`mapping`、`key_calibrations`、`selected_device_id`、`hid_tap_eat`、
    `long_press_ms`（默认 550）、`double_click_ms`（默认 250）、`voice_target`、`shortcuts`、
    `last_update_check_at`（上次成功检查更新的 Unix 秒，自动更新节流用）。
- 触发方式（5 种）：**单击 / 双击 / 长按 / 按下 / 松开**；麦克风只有「按下 / 松开」（PTT）。
- **单击时序**：只有显式绑定了双击的按键才会等双击窗口确认单击；**没有双击绑定的按键在松开
  瞬间立即执行**（快速连按每次都生效，不会被合并成双击吞掉）。绑了不同动作的长按仍按阈值触发一次。
- **动作按下语义**（`ActionKind::press_semantics`）：系统音量 ± 按下即执行一格、按住连发
  （长按阈值后每 120ms）；静音 / 播放暂停按下即执行、不连发；其余动作走手势判定。
  用户显式绑定的双击 / 不同动作长按永远优先。
- **映射表直接操作**：悬停 / 选中已绑定格显示红色 × = 取消关联（删除绑定）；点击格子本体 =
  切换禁用 / 启用。禁用只翻转 `KeyBinding.enabled`（serde 默认 true，旧配置兼容），动作与记录
  保留，行为等同未绑定（不触发、不占手势）；旧「其他 → 禁用」动作启动时迁移为未绑定。
  `get_mappings` 返回空列表表示用户删光了所有绑定，前端**不得回退默认映射**。
- 映射目录由后端 `get_mapping_catalog` 返回：
  - 13 个按键（顺序与前端矩阵一致：电源、麦克风、上、左、确定、右、下、返回、音量+、主页、音量−、菜单、TV）；
  - 5 种触发；2 个动作分类（系统 / 自定义，「自定义」用哨兵值 `custom_combo`）；
  - 默认绑定列表（14 条），前端只负责渲染。
- **应用档案（app profile）**：按前台窗口进程（`process` / `window_title_contains`）匹配一套按键绑定与快捷键。
  - 用户目录：`%LOCALAPPDATA%\RemoteMic\RC003\app-profiles\*.json`；
  - 种子模板：仓库根 `profiles/*.json`，首次启动播种到用户目录；
  - 前端作用域切换在映射页顶部（`pages/mapping/ScopeBar.tsx`）；
  - 后端命令：`app_profile_status / app_profile_catalog / app_menu_apps / create_app_profile /
    save_profile_binding / clear_profile_binding / delete_app_profile / reload_app_profiles`；
  - 写命令均为「读文件 → 改一处 → 原子写回 → 重载调度器」，前端不需要再补 reload；
  - 设计说明见 [docs/design/app-profile-mapping.html](docs/design/app-profile-mapping.html)。
- **自定义快捷键（NamedShortcut）**：名字只是显示层，绑定里存的是 `combo:lctrl+k` 原始写法；
  改 / 删名字不影响已有绑定（显示时回退原始按键）。
- **语音目标 `voice_target`**：默认 `windows_voice`（Win+H）；`ime_wechat / ime_doubao / ime_sogou`
  为预留值（唤起方式待真机验证），命令 `get_voice_target / set_voice_target`。

### 5. 按键来源分类（三类流）

- 当前物理按键来源分三类：

  | 类别 | 链路 | 按键（usage） |
  | --- | --- | --- |
  | ATVV 控制流（非 HID） | BLE `Control` -> `core-voice` | 音频会话事件（AudioStarted/AudioStopped）；麦克风键实际走 HID 兜底 `0x3E` |
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

- 三类流最终汇聚方式：
  - 麦克风键**走调度器映射表**（不是硬编码）：HID 兜底 `0x3E` → vkey 116 → 默认映射
    **Press→Voice、Release→Voice**。Press/Release 是**长按门控**：按住达到长按阈值（默认 550ms）
    才发 Press（Win+H 打开/重置会话），长按结束（松开）才发 Release（Win+H 停止当前会话）；
    快速点按（单击/双击）不产生 Press/Release。用户可在映射页把麦克风改成任意动作。
    旧配置里的 Mic SingleClick 会在启动时自动迁移为 Press/Release。
    ATVV Control 的 `MicButtonPressed` 只计数不切换语音；`AudioStopped` 只停解码不关语音条。
  - 其余 12 键：Raw Input / HOGP 旁路 → `core_dispatch::KeyDispatcher`（每键一个 `TriggerDetector`）
    → 查 `MappingConfig` → `core-input` 注入，并写入 `core-stats`。
  - 映射页保存后通过 `save_mapping` 热更新调度器；诊断页按键测试期间会
    `set_dispatch_enabled(false)` 暂停调度。
  - 「断开」语音桥也会**暂停按键接管**（见 §6.4），点「连接」恢复。
- 菜单键默认映射 `ToggleQuickMenu`（旧 `ContextMenu` 自动迁移）；快捷菜单打开时进入独占输入模式。
- 音频流与按键流是不同线程：音频走 GATT Audio 回调 → channel → 桥接主线程 → `AudioSink`；
  按键 / 控制走 GATT Control 回调或 HID Hook / Raw Input 线程。

#### 5.1 返回 / 音量 / TV 键的信号通道（重要实测结论）

- **usage 标准性**：
  - 音量+ `0x80`、音量− `0x81` 是 HID 键盘页标准 usage（Keyboard Volume Up/Down）。
  - TV `0x35` 是标准 usage（Keyboard Launch Media Select）。
  - 返回 `0xF1` 是**厂商自定义 usage**（HID 键盘页 0xE8–0xFF 为保留区，小米自行使用）。
- **四个键在 Windows 里的命运完全相同**（都属于「应用命令类按键」）：
  - 系统认识并原生处理：返回 → `VK_BROWSER_BACK`、音量 → `VK_VOLUME_UP/DOWN`、
    TV → `VK_LAUNCH_MEDIA_SELECT`，走 WM_APPCOMMAND 应用命令通道（推断，待真机验证）。
  - 它们被 HOGP 层从「键盘输入通道」分流，所以 **Raw Input 和 WH_KEYBOARD_LL 钩子都看不到**（实测）。
  - 但 **Frida 旁路（HOGP 驱动层）四个键都能捕获到**（实测，日志可见 `0x00F1` / `0x0080` / `0x0081` / `0x0035`）。
  - 返回键**没有被系统丢弃**，它和音量/TV 键完全同类，只是 usage 编号是厂商自定义的。
- **主页键 vkey 映射（已修复）**：键盘页 `0x4A` 实测 Windows 映射为 `VK_HOME(36)`，不是
  `VK_BROWSER_HOME(172)`；`usage_to_vkey(0x4A)` 已改为 36（消费类页 `0x0223` 仍映射 172，不同通道）。
- **双重处理问题**：系统原生执行（音量+1 / 浏览器后退）+ 旁路注入映射动作 = 一个信号做两件事。
  现已由驱动层「吃掉」模式解决（见下）。
- **「吃掉」模式（已实现并真机验证，默认开启，通用设置可切换）**：
  真实报告路径是 **GATT 通知 → 8 字节队列项 → `0x1febc` 分配 WDF 请求缓冲区 → `0x20080` 的
  memcpy 拷入 → 完成请求 → 内核 HID 驱动**。Frida 脚本钩住 `0x20080` 这个 memcpy 调用点：
  先把清零前的队列项原始报告转成 9 字节 IOCTL 格式（`01 00 00` + 3 个 usage）以 `gatt_read`
  上报应用调度器，再把源缓冲区清零——系统 HOGP 层看不到任何遥控器按键，只由本应用注入映射动作。
  - **持久化与热切换**：开关存于 `config.json` 的 `hid_tap_eat`（默认 `true`）；后端启动/切换时写入
    `%PROGRAMDATA%\RemoteMic\hid-tap\eat-mode.txt`，Frida 脚本每秒轮询该文件热更新，
    **切换无需重新注入 WUDFHost / 不弹 UAC**。注入时 `frida-gadget.config` 的 `parameters.eat` 只是初始值。
  - **优先级**：`eat-mode.txt`（用户设置）> 环境变量 `REMOTE_MIC_HID_TAP_EAT`（开发兜底）> 默认开启。
  - 注意：eat 开启时如果某键未配映射动作，该键对系统与本应用都无效（信号被应用独家持有）。
  - 旧方案（清 `READ_CHARACTERISTIC_IOCTL` 输出缓冲）已证明无效并删除。
  - 驱动更新导致偏移失效时，按 skill `hogp-report-path-re` 重新定位（`.agent/skills/hogp-report-path-re/SKILL.md`）。
- **已废弃的规划**：曾计划在 WH_KEYBOARD_LL 钩子里拦截普通键（return 1）实现「单一持有者」。
  驱动层 eat 生效后不再需要；且钩子层面无法区分遥控器与 PC 键盘（vkey + scan code 相同）。
- **HOGP 探针代码已收敛**：RE 期诊断噪音已删除；保留的轻量追踪（`report_path` / `memcpy_trace` /
  `vcall_trace`）仅在 `REMOTE_MIC_HID_TAP_TRACE=1` 时输出，用于偏移失效时重新定位。

### 6. 语音桥 / ATVV 生命周期

#### 6.1 事件驱动监督线程（v0.6.0 起，不再无限盲重连）

- 入口：`start_voice_bridge`（`src-tauri/src/commands/voice_bridge.rs`，只是薄适配层）
  启动一个监督线程；`retry_voice_bridge` 用于失败后手动重试；`stop_voice_bridge` 停止。
- **模块边界（重要）**：
  - 状态机与重试策略在 `crates/core-voice/src/supervisor.rs`（`run_supervisor`），
    不依赖 Tauri，通过 `SupervisorHost` trait 向外发状态/链路事件、释放按键，可单独单测；
  - `commands/voice_bridge.rs` 只负责：把 Tauri 事件接到 `SupervisorHost`、启停线程、
    断开时暂停/连接时恢复按键接管；
  - `commands/audio_tools.rs` = 诊断 / 测试音 / 语音链路模拟 / 唤出 Win+H；
  - `commands/vb_cable.rs` = VB-CABLE 检测与一键安装（安装器相关）。
  - 旧的 `commands/audio.rs` 已拆分删除，新增功能请按上述职责放置。
- **端口抽象（ports）**：`core-voice/src/ports.rs` 定义 `VoiceTransport`（传输 /
  GATT 通知 / 写 TX）、`AudioSink`（音频输出）、`KeySink`（虚拟 HID / Win+H）三个 trait；
  真实实现分别在 `transport.rs`（ATVV）、`sink.rs`（CABLE 输出）、`keys.rs`（按键注入）。
  `bridge` 与 `simulate` 都只依赖这组 trait，新增平台/测试假实现时不要再直接
  调用 `core-ble` / `core-audio` / `core-hid` / `core-input` 的具体类型。
- **`core-voice/src/bridge/` 内部拆分**：`mod.rs`（会话编排：连接 → 注册回调 →
  输出泵 → 收尾）、`diag.rs`（断链诊断/包计数/summary）、`handlers.rs`（ATVV 音频/
  控制通知回调：解码 / 捕获 / 诊断 / 输出通道）、`pump.rs`（输出泵：推 sink、失败重建、
  空闲心跳、退出原因）、`report.rs`（会话统计快照）。
- 监督线程第一步打开轻量链路监视器 `core_ble::LinkWatcher`：
  **只监听 `ConnectionStatusChanged`，不打开任何 GATT 服务**，避免抢设备。
- 状态机（`BridgeStatus.state`，前端 `BridgeState` 同名字段）：
  `idle / waiting / starting / running / retrying / failed / stopped`。
  - **waiting**：链路未连接（遥控器不在本机 / 蓝牙关闭 / 被另一台电脑占用），
    `wait_change(500ms)` + 轮询 `is_connected()` 双保险等待系统重连；不会去抢设备。
  - **starting → running**：链路连上后建立一次 ATVV 会话；会话开始即置位端点就绪。
  - **会话随链路断开结束** → 回到 waiting（不盲重试）。
  - 只有「链路仍在但会话失败」才重试：延迟 2/4/8/15/30s + 抖动；连续失败 5 次进入
    **failed** 等待（等链路变化或用户操作，不再空转）。
- 单次会话用 `catch_unwind` 隔离 panic，监督线程不会被带走；会话结束统一
  `force_release_held_keys` 兜底释放按键（链路断了物理松开事件永远到不了）。
- 「链路通但零数据」连续 3 次会进入 failed，并提示给遥控器断电重启（抠电池 10 秒）。
- 前端连接页据此显示等待 / 重连中 / 失败原因 / 重试按钮；`stop_voice_bridge` 的返回文案
  会明确是否真的停下来了（避免「显示已断开、其实还在重连」）。

#### 6.2 ATVV / 音频协议要点

- GATT 服务：`AB5E0001-...`
  - TX：`...0002`
  - Audio：`...0003`
  - Control：`...0004`
- 重要 opcode：
  - `AUDIO_STOP=0x00`、`AUDIO_START=0x04`、`MIC_BUTTON=0x08`、`AUDIO_SYNC=0x0A`、`CAPS=0x0B`
- 音频：IMA/DVI ADPCM 16kHz，高 nibble 优先；默认帧长 `DEFAULT_FRAME_BYTES = 120`，
  实际以 CAPS 协商的 `frame_size` 为准（不同固件可能 120/126/134/160，按协商值切帧）。
- WinRT 异步等待统一走 `core_ble::async_wait::wait_op`（轮询 + 超时 + Cancel），
  禁止 `pollster::block_on` 直接等设备操作（蓝牙栈卡死时会永久阻塞、停不下来）。

#### 6.3 语音输出与 Win+H 绑定

- `AudioSink` 输入固定为语音引擎格式 **48kHz / 2ch / f32**；设备格式不同时自动重采样 / 声道适配
  （`core-audio/src/sink.rs` 的格式协商），不再「格式一变就报错或变调」。
- 语音固定写入 `CABLE Input`（`src/constants.ts` 的 `OUTPUT_DEVICE`，单一事实来源）。
- **Win+H 麦克风绑定机制（重要实测结论）**：
  - Windows 11 语音输入（`TextInputHost.exe`）维护专属的持久化音频偏好，**完全无视系统全局默认
    麦克风的切换**（`IPolicyConfig` 改默认、杀进程冷启动均无效）。
  - 正确架构：后端固定写 `CABLE Input`；首次使用时引导用户按 `Win+H`，在语音条设置中
    **手动把麦克风选为 `CABLE Output`**（仅需一次，Windows 永久记忆）。
  - 这样电脑原本的物理麦克风保持全局默认，开会 / 微信通话不受影响。
  - **禁止**在语音链路里做不可靠的「动态改全局默认麦克风」。
- 语音优先走 Windows 自带语音输入（Win+H）。
- **Win+H 按键语义（实测）**：首次按打开语音条；弹窗已打开时再按**不会关闭**，只停止/重置当前输入会话；
  关闭弹窗用 Esc 或点 ✕。麦克风键 Press/Release 各发一次 Win+H（长按识别后 Press、松开 Release）。
- `trigger_voice_typing` 命令：唤起 Win+H 并提示把麦克风选为 `CABLE Output`（引导页使用）。

#### 6.4 「断开」的语义（v0.6.2 起）

- 「断开」= **停止语音桥 + 暂停遥控器按键接管**（`dispatcher.set_enabled(false)`）：
  断开后按遥控器按键不会再触发映射、也不会再唤起豆包等输入法。
- 点「连接」/「重试」会恢复按键接管（`set_enabled(true)`），避免「连上了但按键没反应」。
- 注意：**Windows 的 HOGP 键盘链路不会（也无法）被本应用断开**。界面上的「未连接」指本应用的
  语音桥 / 设备状态，不是系统蓝牙断开；要彻底断开遥控器，只能去 Windows 蓝牙设置删除设备。
  连接页有对应提示块，别再把它当 bug。
- 停止返回文案：`已断开连接（遥控器按键接管已暂停，点「连接」恢复）`。

#### 6.5 会话诊断与静默会话

- 每次会话结束写一条诊断日志，字段：`会话时长 / 距上次控制通知 / 距上次音频 / 距上次麦克风事件 /
  控制包 / 音频包 / 语音输入中 / session_active / exit_reason`。
- `core_voice::last_session_report()` 返回最近一次会话快照（duration / control_pkts / audio_pkts / exit_reason），
  监督线程用它判断「静默会话」：`duration >= 15s && control_pkts == 0`（阈值见 `is_silent_session`）。
- 静默会话告警只用于提示遥控器语音服务可能挂起；注意 CAPS 响应有时会延迟（见 §10 已知问题），
  该判定可能误报。

### 7. 日志与数据文件

- 统一写入 `%LOCALAPPDATA%\RemoteMic\RC003\remote-mic.log`，格式 `[时间] [级别] 内容`：
  `DEBUG`（默认关）/ `INFO` / `WARN` / `ERROR`。
- 临时开启 DEBUG（实时生效，无需重启）：
  - 环境变量 `REMOTE_MIC_DEBUG=1`
  - 创建文件 `%LOCALAPPDATA%\RemoteMic\RC003\debug`
  - 诊断页「开启 DEBUG」按钮（`core_log::set_debug_enabled`）
- 日志轮转：超过 `MAX_LOG_BYTES = 2 MiB` 改名为 `remote-mic.<时间戳>.log`，保留 5 份备份。
- 日志接口统一走 `core-log`：`log_debug / log_line / log_info / log_warn / log_error`；
  读取 / 清空 / 列表 / DEBUG 开关也在 core-log。不要用 `eprintln!` / `println!` 输出调试信息。
- 其他数据文件（均在 `%LOCALAPPDATA%\RemoteMic\RC003\`）：
  - `config.json`（+ `config.json.bak` 回退）
  - `app-profiles/*.json`（应用档案）
  - `stats.json`（本机统计，批量写盘；统计不是关键数据）
  - `captures/`（会话原始音频/控制帧录制，用于排障）
  - `panic.log`（panic 钩子写入的崩溃信息）
  - `%PROGRAMDATA%\RemoteMic\hid-tap\eat-mode.txt`（eat 模式热切换标志）
- 进程为单实例：`core_input::acquire_single_instance()` 失败会直接退出，避免双开抢设备。
- 常用筛选（PowerShell）：

  ```powershell
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "\[ERROR\]"
  Select-String -Path "$env:LOCALAPPDATA\RemoteMic\RC003\remote-mic.log" -Pattern "断链诊断|静默|零数据"
  ```

### 8. 诊断 / 引导 / 通用设置

- 诊断页「状态与自检」共 10 项：
  音频端点枚举、VB-CABLE 虚拟声卡、配置目录可写、日志目录可写、虚拟 HID 键盘、ATVV→ADPCM→输出帧、
  测试音播放（CABLE 验证）、长按触发条件识别、本地统计读写、最近语音会话健康。
- VB-CABLE 一键安装（诊断页）：
  - 运行安装目录下 `scripts/install-vb-cable.ps1`：从 VB-Audio 官网下载官方安装包、校验固定 SHA-256、
    解压到 `%LOCALAPPDATA%`，再以管理员权限启动**官方安装程序**；
  - 安装后**必须重启电脑**，回诊断页点「重新检测」；
  - 需要网络 + 管理员权限；VB-CABLE 本体不随包分发（EULA 限制，见 §9）。
- 应用内还可重新运行虚拟 HID 驱动安装（`install_vhid_driver`，用于修复 / 重装）。
- 引导页 4 步：1) 蓝牙配对；2) 确认虚拟声卡驱动；3) 关键：Win+H 里把麦克风绑定为 CABLE Output；
  4) 模拟自检与日常使用。
- 通用设置：拦截 HID 按键信号（eat 模式）、长按判定、双击间隔、「关于与更新」（检查 / 下载 / 安装更新）。

### 9. 打包与发版

- **安装包内容**（NSIS，`perMachine`）：应用本体 + WebView2 bootstrapper + 虚拟 HID 驱动 bundle
  （`core_input` 的 WinUHid）+ VB-CABLE **助手脚本**（不是 VB-CABLE 本体）。
- `tauri.conf.json` 的 `beforeBuildCommand` = `npm run build:installer`（前端构建 + stage VB-CABLE 脚本）；
  完整包另需先运行 `scripts/prepare-vhid-bundle.ps1` 生成 `src-tauri/windows/driver/`。
- `hooks.nsh` 在安装时：把驱动复制到 `$INSTDIR\vhid` 并调用安装 / 卸载脚本（一次 UAC 完成应用与驱动安装），
  把 VB-CABLE 助手脚本复制到 `$INSTDIR\scripts` 供应用内一键安装使用。
- 驱动为**测试签名**：安装器会把随包证书装入本机信任库；首次连接遥控器时 HOGP 旁路注入可能再弹一次 UAC。
- **VB-CABLE 许可约束**：其 EULA 允许 AS IS 再分发，但禁止集成进其他产品的安装流程；
  因此仓库只分发自己的下载/校验助手脚本 + `scripts/vb-cable.sha256`（固定官方包 SHA-256），
  用户触发后才下载官方安装包。**不要把官方安装包提交进仓库或塞进安装器。**
- **自动更新（Tauri updater）**：`bundle.createUpdaterArtifacts = true` + `plugins.updater`
  （公钥 / `releases/latest/download/latest.json` endpoint，`installMode = passive`）。
  发布时用 minisign 密钥给安装包签名；私钥与密码放在 GitHub Secrets
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`，**不要提交到仓库**。
  密钥生成 / 备份 / 轮换风险与排查见 [docs/项目/自动更新.md](docs/项目/自动更新.md)。
- 发版流程（详见 `CHANGELOG.md` 顶部「发版流程」）：
  1. `main` 通过 CI；
  2. 三处版本号同步：`package.json`、`src-tauri/tauri.conf.json`、`Cargo.toml` 的 `workspace.package.version`；
  3. 更新 `CHANGELOG.md`（`[Unreleased]` 内容移入新版本小节；发布工作流会把它写进 `latest.json` 的 notes）；
  4. 跑 `cargo check -p remote-mic` 让 `Cargo.lock` 跟着更新；
  5. 通过 PR 合入 `main`（受保护分支）；
  6. 打 tag 并推送：`git tag -a vX.Y.Z -m "..." && git push origin vX.Y.Z`；
  7. `Release Windows Build` 工作流自动构建并创建 GitHub Release（也可 workflow_dispatch 手动填 tag）；
  8. 工作流同时生成并上传 `latest.json` 与 `.exe.sig` —— **每个正式 Release 都必须带 `latest.json`**，
     否则老客户端检查更新会 404；
  9. 下载产物核对：`sha256sum -c SHA256SUMS.txt` + 核对 exe 的 FileVersion。
- 发布工作流关键步骤：准备 / 校验驱动 bundle → 准备 / 校验 VB-CABLE 脚本 → 带 updater 签名构建
  （没有密钥时自动退化为普通构建）→ 校验 `.exe.sig` → 用 7-Zip 解包验证 VB-CABLE 脚本确实嵌入
  → 收集产物并生成 `SHA256SUMS.txt` → 生成 `latest.json` → 创建 Release → 补传 `.sig` / `latest.json`。
- 产物命名：`Remote.Mic_<ver>_x64-setup.exe`、`remote-mic.exe`、`SHA256SUMS.txt`、
  `Remote.Mic_<ver>_x64-setup.exe.sig`、`latest.json`。
- 当前**未做 Authenticode 代码签名**；自动更新签名（minisign）与 Authenticode 无关，已在上面说明。
  后续可接 Authenticode 签名。

### 10. 已知问题 / 待办（截至 2026-10-20，v0.6.2）

- **ATVV 通知偶发丢失（未修复，排查中）**：部分「断链重连后新建」的会话里，HID 按键正常
  （能唤出豆包 / Win+H），但 ATVV 控制 / 音频通知收不到（`控制包=0, 音频包=0`），
  且 CAPS / AudioStopped 往往在会话结束、句柄释放时才被 flush 出来；同时观察到会话约 42–45s
  空闲断链后循环。应用重启后的首个会话常常正常，可作临时绕过。
  已试但未合入的方向：`GattSession.MaintainConnection`、会话强制 Uncached、CCCD 先 None 再 Notify、
  以及「链路断开期间保持同一 GATT 会话」。
- 静默会话判定（`duration>=15s && 控制包==0`）可能误报：正常会话的 CAPS 也可能延迟到断开才收到。
  后续可改为「用户按下麦克风后 2s 内无音频」这类更直接的判定。
- ATVV `MIC_OPEN / MIC_CLOSE` 握手未实现；`MIC_EXTEND` 长语音未支持。
- 输出端点用户可选 / 持久化未做（当前固定 `CABLE Input`）。
- 代码签名、托盘、i18n、隐私说明未做；自动更新已接入（见 §9 与 [docs/项目/自动更新.md](docs/项目/自动更新.md)），
  真机全链路验收待补（见 `docs/项目/真机验收.md` §7）。
- 任务清单持续维护在 [docs/项目/任务清单.md](docs/项目/任务清单.md)；
  容错轮次与审计结论在 [docs/项目/容错能力评估.md](docs/项目/容错能力评估.md)。

## 约定与注意事项

- **UI 全部使用简体中文。**
- **前端结构与样式**：页面样式放 `src/pages/<Page>.css`（随页面组件按需加载），
  共享的布局 / 表单 / 控件样式留在 `src/App.css`；页面逻辑复杂时抽成
  `src/pages/<page>/use*.ts` hooks（参考 `pages/connection/useConnectionActions.ts`、
  `useVbCableDriver.ts`），页面组件只负责渲染。拆分样式时保持规则原文不动，
  用「拆分前后规则集合一致」做回归检查（本次 260 条规则一致、无跨文件重复选择器）。
- **crate 边界**：
  - `core-hid` 只负责 HID 底层事件捕获 / 报告解析（Raw Input、HID 报告、HOGP 旁路）。
    `tap/` 模块化为：`config.rs`（配置/路径）、`hub.rs`（IPC 通信与分发）、`inject.rs`（提权注入）、`runtime.rs`（启动与生命周期）、`status.rs`（状态上报）、`watchdog.rs`（按住跟踪与兜底释放）。
  - `core-app-profile` 负责前台应用检测、种子落地与应用专属映射文件管理。
    内部模块化为：`seeds.rs`（种子嵌入与落地清单）、`store.rs`（磁盘原子读写与安全 id 生成）、`registry.rs`（内存注册表与前台窗口匹配）、`foreground.rs`（前台进程/标题查询）、`window.rs`（窗口激活与启动）、`tests.rs`。
  - `core-input` 只负责 Windows 输入注入、热键、输入钩子 / 动作执行。
    `vhid.rs` 内部模块化为：`vhid.rs`（驱动动态加载与报告提交）、`vhid/diag.rs`（Secure Boot / HVCI 注册表探测与环境建议）、`vhid/tests.rs`。
  - `core-mapping` 只负责按键 / 触发 / 动作的纯逻辑定义。
    内部模块化为：`combo.rs`（快捷键规范化与解析）、`config.rs`（映射配置结构与默认映射表）、`trigger.rs`（按键手势判定状态机）、`tests.rs`。
  - `core-dispatch` 负责把物理按键接到映射表并执行动作；不要把 SendInput 或 Raw Input 细节塞回 mapping。
    内部再分：`lib.rs`（触发判定 / 调度状态机）、`executor.rs`（动作执行线程）、`tests.rs`；
    新增「动作怎么执行」放 executor，新增「何时触发」放 lib。
  - `src-tauri/src/support/` 只放与 Tauri 生命周期无关的通用支撑（路径 / 状态 / 按键兜底 /
    panic / 映射编解码）；命令、窗口、调度器装配留在 `lib.rs`。
  - 日志统一以 `core-log` 为唯一出口；`core-input` 现有的 `log_*` 薄封装仅为兼容旧调用，新代码不要再增加包装。
- **单一事实来源**：运行时状态、端点就绪、输出设备、映射目录都以 §3 列出的后端字段 / 常量为准，
  前端不要再造副本。
- `scripts/*.ps1` 保持纯 ASCII，避免 PowerShell 编码问题。
- Windows PowerShell 5.1 没有 `Join-String`，需要用 `-join`。
- WSL 与 Windows 共享 node_modules 会产生平台冲突；如遇 esbuild/rollup 平台错误，在 Windows 端重新 `npm install`。
- Vite dev 端口固定为 `1420`；启动前确认端口未被占用。
- Rust 编译请使用 Windows 端 `cargo`；WSL 直接交叉检查 Windows target 可能缺 `llvm-rc`。
- 不要提交生成的 `src-tauri/windows/driver/`、`src-tauri/windows/vb-cable/`（均 gitignored）。
- **UI 改动必须真机验证**：改完在真实运行的窗口里看过再汇报，并把关键状态截图放到
  `docs/项目/截图/`（命名：`页面-功能.png`），不要只凭构建通过下结论。
- 文档与验证记录用中文；涉及设计的改动优先补 `docs/` 下对应文档。

## 分支 / PR / 提交规范

- 分支策略（**一个会话 = 一个分支 = 一个正在开发的功能**）：
   - `main` 为受保护主干，始终应保持可构建、可运行。
   - 每次开始一个新任务 / 新功能 / 一次集中重构，从 `main` 切**一个**长期分支，
     命名如 `feat/xxx`、`fix/xxx`、`docs/xxx`、`refactor/xxx`；这个会话（任务）里的所有相关改动
     （代码、文档、测试、验证记录）都提交到**这一个**分支上，最后只开**一个** PR。
   - 不要把同一个任务拆成多个并行分支或栈式 PR；子任务在分支内用普通提交表达。
   - 确实要同时开发两个互不相关的功能时，另开会话/分支；一个分支同一时间只做一件事。
   - 完成并通过本地验证后，通过 Pull Request 提交审核（文档改动也走 PR）。
- PR 要求：
   - 标题简明，概括这个分支（会话）的完整改动。
   - 描述关联的 issue / 任务项（如有）。
   - 必须通过 CI：前端 typecheck / build、Rust fmt / clippy / test、Tauri check。
   - 涉及 UI 改动时附上改动说明或截图。
- **合并必须经用户明确批准**：
   - PR 只代表「开发完成、等待审核」，**不得自动合并**；
   - 只有用户明确说「合并 / 合入 / 可以合」后，才执行 `gh pr merge`；
   - 合并后删除该会话分支；发版 / 打 tag 同理，必须用户明确要求才执行。
- 提交信息规范（Conventional Commits 风格）：
  - `feat: 新功能`
  - `fix: 修复问题`
  - `docs: 文档变更`
  - `chore: 构建 / 工具 / 依赖等杂项`
  - `refactor: 重构`
  - `test: 测试相关`
- 发版见 §9 / `CHANGELOG.md`：更新版本号 → 更新 CHANGELOG → PR 合入 → 打 tag → CI 自动出 Release。

## 工作流提示

- 修改前端后至少运行 `npm run typecheck`，完整验证用 `npm run build`。
- 修改 Rust 后运行：
  ```powershell
  cargo fmt --all -- --check
  cargo clippy --workspace --exclude remote-mic --all-targets -- -D warnings
  cargo check -p remote-mic
  cargo test --workspace --exclude remote-mic
  cargo test -p remote-mic
  ```
- 提交前检查 `git status`，避免把临时文件或平台相关 node_modules 提交进去。
- **工具链与 CI 对齐**：CI 使用最新 stable（`windows-latest`），本地 rustc 落后时可能因
  新增 clippy lint 在 CI 翻红（曾遇 1.99 的 `chunks_exact_to_as_chunks`）。发版前先
  `rustup update stable` 对齐。
- 需要重启 Tauri dev 时，先结束旧 `remote-mic.exe` / 占用 `1420` 的进程，再重新启动。
- **前端改动看不见？先确认应用是怎么起的。**（踩过一次，浪费好几轮）
  - `npm run tauri dev` → 走 Vite 开发服务器（`build.devUrl`），改 CSS/TSX **即时热更新**，端口 1420 在监听。
  - `cargo run` → **没有开发服务器**，Tauri 把 `dist/` 在**编译期**烤进二进制（`build.frontendDist`）。
    此时改前端必须 `npm run build` **再重编 Rust 再重启应用**才生效。
  - 判别方法：`Get-NetTCPConnection -LocalPort 1420 -State Listen` 有没有结果；
    再比 `remote-mic.exe` 的编译时间与 `dist/` 的构建时间——exe 更早就说明前端是旧的。
  - UI 改动**必须在真的那一个实例上看过**再汇报，不能只凭无头浏览器或构建通过就下结论。
- 真机截图可用 Windows PowerShell 枚举窗口 + `CopyFromScreen`（注意进程可能同时有多个辅助窗口，
  取可见且面积最大的那个；DPI 感知需开 `SetProcessDpiAwarenessContext(-4)`）。
- **HOGP 旁路 / 吃掉模式失效时**：先调用 skill `hogp-report-path-re`
  （`.agent/skills/hogp-report-path-re/SKILL.md`），按其中的方法论与 Playbook 重新定位驱动报告写入点；
  不要凭旧偏移盲改。
- 发版后核对：`gh run watch <run-id> --exit-status` → GitHub Release 资产齐全 →
  `sha256sum -c SHA256SUMS.txt` → `remote-mic.exe` 的 `FileVersion` 与 tag 一致。
