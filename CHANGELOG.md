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

### 新增
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
