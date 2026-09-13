# Changelog

本项目的版本变更记录。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## 发版流程

发版 = 更新版本号 + 打 tag → CI 自动出 Release。

1. 确认 `main` 分支通过 CI（`npm run typecheck`、`cargo test`、`cargo clippy`、`cargo check`）。
2. 更新版本号，三处保持一致：
   - `package.json` 的 `version`
   - `src-tauri/tauri.conf.json` 的 `version`
   - `Cargo.toml` 的 `workspace.package.version`
3. 把本次变更写入本文件，并提交（如 `chore: release v0.1.0`）。
4. 推送后打 tag：`git tag v0.1.0 && git push origin v0.1.0`。
5. GitHub Actions `Release Windows Build` 自动构建安装包、生成 `SHA256SUMS.txt`、创建 GitHub Release 并上传产物。
6. 到 GitHub Releases 页面核对安装包与校验和。

> 手动触发时：进入 Actions → `Release Windows Build` → `Run workflow`，填写 tag 名称。构建产物与自动打 tag 流程一致。

## [Unreleased]

## [0.3.0] - 2026-09-13

### 新增
- 安装包内置虚拟 HID 键盘驱动，普通用户不再需要手动跑脚本：
  - 新增 `scripts/prepare-vhid-bundle.ps1`：一键汇编驱动包（编 UMDF 驱动 → stampinf/Inf2Cat/签名 → 连同 `devcon.exe`、安装/卸载脚本、测试证书输出到 `src-tauri/windows/driver/`，该目录已加入 `.gitignore`）。
  - 新增 `src-tauri/windows/hooks.nsh`：NSIS 安装钩子在 `POSTINSTALL` 把驱动包释放到 `$INSTDIR\vhid` 并执行安装，`PREUNINSTALL` 移除 `Root\WinUHid` 设备与 DriverStore 条目；驱动缺失时不阻断构建（`File /nonfatal` + 运行时 `IfFileExists` 双重保护）。
  - 安装模式由 `currentUser` 改为 `perMachine`：安装器以管理员运行，一次 UAC 内完成应用与驱动的安装。
  - `install-winuhid.ps1` 改为接收 `-DriverDir` 参数并记录驱动已发布名（`oemNN.inf`）；新增 `uninstall-winuhid.ps1` 按记录清理，避免 DriverStore 残留。
  - 诊断页新增「安装 / 修复虚拟键盘驱动」按钮（`install_vhid_driver` 命令），复用安装包内的同一套脚本，可随时修复。
  - Release 工作流在构建安装包前汇编驱动包并校验产物完整，避免发布出缺少驱动的安装包。

## [0.2.0] - 2026-09-13

### 新增
- **按键注入全部改走虚拟 HID 键盘**：
  - 新增 `crates/core-input/src/vhid.rs` + `hid_kbd.rs`：经 WinUHid（UMDF + 收件箱 `vhf.sys`）创建标准 Boot Keyboard，提交 8 字节 HID 报告；左右修饰键分开编码（右 Ctrl 为修饰位 `0x10`）。按键从此在系统里等同真实键盘——豆包等只认真实 HID 键盘的输入法可以正常响应。
  - 媒体键单独一台 Consumer Control 虚拟设备（播放/暂停、上/下一曲、停止）。HID 键盘页（`0x07`）没有这些 usage，必须走 Consumer 页（`0x0C`）。
  - `send_key_combo` / `send_key_down` / `send_key_up` / `press_win_h` / `press_escape` / `open_voice_typing` 全部走虚拟 HID；`hotkey.rs` 里的 SendInput 注入实现已删除，不再存在绕过虚拟 HID 的注入路径。
  - 驱动构建与安装脚本：`scripts/build-winuhid.ps1`（用 WDK NuGet 编用户态 `WinUHid.dll`）、`build-winuhid-driver.ps1`、`package-winuhid-driver.ps1`（stampinf + Inf2Cat + 测试签名）、`install-winuhid.ps1`（信任证书、`pnputil` 装包、`devcon` 建 `Root\WinUHid` 设备）、`check-winuhid.ps1`。虚拟 HID 目录已加入 `.gitignore`。
  - 诊断页自检新增「虚拟 HID 键盘」项（`core_input::vhid_probe`：查 `WinUHid.dll` 与 `\\.\WinUHid` 控制设备）。
- 连接页「断开连接」：已连接时「连接」按钮变为「断开」，扫描按钮禁用；`stop_voice_bridge` 改为立即请求主循环退出并释放 GATT，不再等下一个重连周期。
- 自定义快捷键（组合键）映射：支持单键与左右 Ctrl / Shift / Alt / Win 区分，映射页聚焦输入框直接按键录制，按 `keyup` 判定完成。
- 新增独立日志页（`src/pages/LogPage.tsx`）与侧栏入口。
- 驱动层「拦截 HID 按键信号」模式（默认开启）：
  - 逆向定位 WUDFHost HOGP 驱动真实报告写入点（GATT 通知 → 队列项 → `0x20080` memcpy），Frida 钩住写入点清零源缓冲区，系统看不到遥控器原始按键，由本应用独家注入映射动作，消除「系统原生动作 + 应用映射动作」双重触发。
  - 连接页新增「拦截 HID 按键信号」开关（`get_hid_tap_eat` / `set_hid_tap_eat`），持久化到 `config.json` 的 `hid_tap_eat`（默认 `true`）；切换热生效，无需重新注入 WUDFHost / 不弹 UAC。
  - Frida 脚本每秒轮询 `%PROGRAMDATA%\RemoteMic\hid-tap\eat-mode.txt` 热更新；优先级：文件 > 环境变量 `REMOTE_MIC_HID_TAP_EAT` > 默认开启。
  - 新增 HOGP 报告路径逆向重定位技能文档 `.agent/skills/hogp-report-path-re/SKILL.md`。
- 麦克风键接入映射表（不再硬编码）：HID `0x3E` → vkey 116 进 `vkey_map`，默认映射为 **Press→Voice、Release→Voice**，可在映射页改为任意动作，支持第三方语音助手。
  - Press/Release 为长按门控：按住达到长按阈值才发 Press，长按结束才发 Release，快速点按不触发。
  - 旧配置 Mic SingleClick 启动时自动迁移为 Press/Release。
- 触发时间可配置：`long_press_ms`（默认 550ms）、`double_click_ms`（默认 300ms）持久化到 `config.json`，映射页可调，保存后热更新调度器（`set_trigger_timing`）。
- 菜单键默认动作改为快捷菜单开关（`ToggleQuickMenu`），旧 `ContextMenu` 配置启动时自动迁移。
- 快捷菜单增强：
  - 打开快捷菜单时进入**菜单独占输入模式**（`InputMode::QuickMenu`），遥控器方向/确定/菜单/返回等按键直接路由给快捷菜单窗口（`quick-menu-key` 事件），不触发普通按键映射；关闭后恢复普通模式。
  - 快捷菜单停留位置与所选环状态持久化到 `localStorage`，下次打开恢复。
  - `public/quick-menu.html` 接入遥控器按键直通事件监听与按键连发。
- 语音桥防并发：新增 `stop_voice_bridge` Tauri 命令；`start_voice_bridge` 原子互斥防止并发双桥争用 GATT，重连等待可响应停止请求。
- 按键映射运行时闭环：新增 `core-dispatch` 调度器（单击/双击/长按 → 查映射 → SendInput），映射保存后热更新；诊断页按键测试时自动暂停调度。
- HOGP 旁路状态改为结构化枚举（`idle` / `pending` / `attached` / `unavailable`），前端不再靠中文消息关键字推断。
- 日志功能补齐：
  - `core-log` 自动轮转（超过 2 MiB 轮转，保留 5 份备份）。
  - 诊断页新增日志面板：查看尾部、清空日志、打开日志目录、切换 DEBUG。
  - 新增 Tauri 命令：`get_log_info` / `read_log_tail` / `clear_log` / `open_log_dir` / `set_debug_logging`。
- 发布流程升级：GitHub Actions 自动创建 Release、上传安装包、生成 SHA256。
- 新增 `docs/项目/真机验收.md` 真机验收记录表。

### 变更
- 连接页改为单栏布局：设备卡 + 语音链路卡（虚拟声卡选择 + 状态），移除识别目标选项与语音测试区（「唤出语音条」「模拟语音链」及其输入框），过程提示不再占用卡片。
- 诊断页移除「逐键校准」模式，只保留按键快速测试（单击/双击/长按矩阵）。
- 移除未使用的 `src/voiceTarget.ts`；`hotkey.rs` 收敛为仅剩 `open_app`。
- 长按触发简化为只触发一次，移除「按住连发」逻辑；长按阈值与双击窗口改为可配置。
- HOGP 旁路看门狗超时 150ms → 2000ms，修复长按被提前截断为单击的问题。
- HOGP 看门狗对麦克风键（`0x3E`）禁用自动释放：长按麦克风期间遥控器不重复发 HID 报告（只推 ATVV 音频），此前 2s 超时会把 Release 提前触发，导致长按中第二次 Win+H 取消语音输入；现在由真实 HID 松开报告或 ATVV AudioStopped 结束长按。
- `AGENTS.md` 移除 clean-room / 外部仓库引用限制。
- HOGP 探针诊断代码收敛：删除模块/导入枚举、函数表 dump、IOCTL 全量追踪、反汇编/调用栈上报等噪音，轻量追踪仅在 `REMOTE_MIC_HID_TAP_TRACE=1` 时输出。
- 清理死代码：移除旧的 `READ_CHARACTERISTIC_IOCTL` 清缓冲方案（已证无效）、语音切换状态机、未使用的音频端点/诊断命令、`VoiceMode`、`action_allows_repeat` 等。
- 同步维护任务清单与规划文档，删除未实际使用的技术栈描述。

## [0.1.0] - 未发布

### 新增
- 初始版本：Tauri 2 + React 桌面壳。
- BLE 扫描 / GATT / ATVV 端点发现。
- ATVV 协议状态机 + IMA/DVI ADPCM 解码。
- WASAPI 音频端点、重采样、VB-CABLE 路由与诊断。
- 按键映射（13 键、单击/双击/长按）。
- HID Raw Input / 低层钩子 / HOGP 旁路捕获框架。
- 语音桥（BLE → ADPCM → 48kHz 立体声输出）。
- 快捷菜单窗口（`public/quick-menu.html`）。
- GitHub Actions CI 与 NSIS 打包配置。
