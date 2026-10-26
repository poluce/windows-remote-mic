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
- **智能体接入（MCP）**：通用设置新增「允许智能体配置（MCP）」开关，默认关闭。
  开启后应用在 `127.0.0.1` 随机端口提供本地 JSON-RPC，并生成一次性 token 写入
  `%LOCALAPPDATA%\RemoteMic\RC003\agent-api.json`；随安装包分发 `remote-mic-mcp.exe`
  （stdio MCP sidecar），支持 MCP 2026-07-28（modern，无 `initialize`）与 2025-11-25（legacy）
  双 era、`server/discover`、12 个映射/触发/状态工具。关闭开关立即停止接口并删除 token。
- 配置新增 `revision`（写版本号）与 `agent_api_enabled`；所有配置写操作收口到
  `ConfigStore::update`（进程内写锁 + revision 自增），UI 与智能体并发修改不再互相覆盖。
- 映射写操作抽出 service 层（`services::mapping`），Tauri command 与 MCP 共用同一套逻辑；
  智能体修改后 emit `config-changed`，映射页自动刷新。

### 构建
- 新增 `crates/mcp-bridge`（`remote-mic-mcp.exe`）与 `scripts/prepare-mcp-sidecar.ps1`；
  安装包通过 `tauri.bundle.conf.json` 的 `externalBin` 携带 sidecar。
  打完整安装包用 `npm run tauri:build`（普通 `npm run tauri build` 不携带 sidecar）。

### 修复
- MCP 接入自查修复：JSON-RPC notification 不再误回响应；重复删除 / 重复设置同一状态不写盘、
  不递增 revision；开启 MCP 失败时的提示不再被折叠在开关关闭态里；智能体修改配置后清空过期的
  撤销 Toast，避免撤销覆盖智能体的新改动。
- 宽窗口下内容区布局：滚动容器从内容列（936px）上移到全宽主区域，内容列水平居中、
  滚动条贴窗口右缘。此前窗口拉宽 / 最大化后，内容列停在左侧、右侧留出一大块空白，
  滚动条也停在窗口中间；切页回到顶部的滚动复位同步改到新的滚动容器。

## [0.7.4] - 2026-10-25

### 新增
- 按键映射表支持直接「取消关联 / 禁用」：悬停或选中已绑定格子时右上角出现红色 ×，
  点 × 删除这条绑定；点击格子本体切换禁用（红色填充）/启用（文字微绿）。
  禁用只翻转 `enabled` 标志、保留动作与记录，再点一下即可恢复；单格删除给 Toast + 撤销。
- 新增全局映射命令 `clear_mapping_binding` / `set_mapping_binding_enabled`；配置项
  `KeyBinding.enabled`（serde 默认 true，旧配置自动兼容）。旧版「其他 → 禁用」动作在启动时
  迁移为未绑定。

### 变更
- 单击响应时序调整：**没有双击绑定的按键，单击在松开瞬间立即生效**（不再等双击窗口），
  快速连按每次都触发；只有显式绑定双击的按键才保留延迟确认。这样默认映射的响应更快，
  也不会再出现「快速连按被合并成双击吞掉」。
- 双击窗口默认值 300ms → 250ms（通用设置仍可调），设置页补充说明：只有绑定双击的键才受该窗口影响。
- 动作分类由 3 个减为 2 个（系统 / 自定义）：「其他 → 禁用」由映射表的禁用开关取代。

### 修复
- 映射表右上角 × 的位置与间距：原先贴整列最右侧会溢出卡片、被裁切并把矩阵撑出横向滚动条，
  贴文字又会压字；现固定在**整格内侧右上角**，格子右侧预留 28px，矩阵禁止横向溢出。
- 映射表把所有绑定删光后，界面不再回退显示默认映射（此前会出现「界面显示默认、调度器实际为空」的不一致）。
- 关闭主窗口后应用不退出、驻留后台且任务栏看不到入口：主窗口收到关闭请求时显式退出应用
  （此前启动时创建的隐藏 `quick-menu` 窗口会让 Tauri 认为仍有窗口存在，事件循环不结束）；
  退出路径仍走 `RunEvent::Exit`，由那里兜底释放虚拟 HID 上可能残留的按住状态。
- 修复「音量 + / 音量 − / 静音」映射动作无效果（日志显示已执行，但系统音量不变）：
  这三个动作此前被注入虚拟 HID 键盘的 Keyboard 页 usage `0x7F/0x80/0x81`，而 Boot Keyboard
  描述符只声明到 `0x65`，超出范围的 usage 会被 Windows HID 栈静默忽略；现改走已有的
  Consumer Control 虚拟设备（Volume Increment / Decrement / Mute `0xE9/0xEA/0xE2`），
  日志中可见「虚拟 HID 媒体键设备已就绪」与 `按住 volume_up report=[10]`。
- 注入层增加「目标设备为准」的校验：虚拟键盘 `add_key` 对超出 `KEYBOARD_USAGE_MAX`
  （Boot Keyboard 声明范围 `0x00–0x65`）的 usage 直接报错，不再静默提交；
  描述符上限与校验常量同源，并补单测锁定（越界 usage、音量 token 不进键盘设备）。
- 修复音量键「快速连按部分按键无反应、按住不连发」：步进类动作（系统音量 ±）改为
  按下立即执行一格、按住超过长按阈值后每 120ms 连发，且不再参与单击/双击手势判定
  （此前快速连按会被合并成双击，而音量键没有双击绑定 → 静默丢弃）；静音 / 播放暂停
  等开关类动作同样改为按下立即执行。动作语义由 `ActionKind::press_semantics()` 统一声明。
  **显式手势绑定优先**：用户给该键绑了双击、或绑了不同动作的长按时，自动退回正常手势判定，
  不会吞掉用户配置的双击/长按；「长按也是音量+」这类同动作冗余配置仍保持连发。

## [0.7.3] - 2026-10-24

### 新增
- **全应用视觉规范统一与材质升级**：
  - 设计 Token：新增 `src/styles/tokens.css`，收敛整套设计变量（`--rm-*`：表面、冷灰调边框、文字四级、圆角/阴影/动效阶梯）；
  - 外壳与材质：雾面质感背景（渐变光斑 + 噪点）、毛玻璃侧栏（`backdrop-filter`，不支持环境优雅退化）、带层次感的纵向微渐变磨砂卡片（双层冷调轮廓 + 接触阴影）；
  - 色板规范：强调色与语义色低饱和调优，页面大面积柔底配合控件层次，文字对比度全量满足 WCAG AA 规范；
  - 控件与导航：侧栏改用单色线性图标与白玻璃胶囊指示条；统一胶囊样式（`.pill`）；补齐按钮激活态（`.btn.on`）；原生下拉移除系统默认箭头对齐自定义样式；
  - 连接页引导卡：设备卡与语音链路下方新增动态「下一步」卡片（未连接时指引配对上手，已连接推流就绪时引导测试与绑定麦克风）；
  - 日志页：拆分时间、日志级别与内容三列，按级别着色并等宽数字对齐；
- **深色模式支持**：
  - 支持「跟随系统 / 浅色 / 深色」三档外观切换（位于「通用设置 → 外观」），偏好存储于本地 `localStorage`，启动首帧即生效避免闪烁；
  - 基于 CSS Token 统一映射，支持原生控件深色模式（`color-scheme: dark`）。

### 修复
- 修复长页面切换到其他导航页时未复位页面滚动条的问题（切换页面自动回滚至顶部）；
- 修复诊断页状态标签被较长内容挤压换行竖排的布局缺陷。

## [0.7.2] - 2026-10-23

### 修复
- 按键映射页：移除遥控器外部多余的 `.card` 白色长条卡片样式，消除内边距对可用空间的挤压，解决遥控器上下圆角被平切截断的问题；
- 语音输入时长根因修复：
  - `core-hid/src/tap/watchdog.rs`：新增音频流心跳续期机制（`touch_mic_activity`），只要遥控器仍在推流语音包，看门狗自动保活麦克风按键，彻底消除 10 秒无 HID 报告即强制掐断的问题；
  - `core-voice`：音频帧到达时通过 `KeySink::touch_mic_activity` 实时联动喂狗；
  - `core-dispatch/src/lib.rs`：麦克风按键（`ButtonId::Mic`）支持长语音输入，即使映射为第三方输入法快捷键（如豆包），也不再受默认 10s 组合键超时掐断，保障长语音正常听写。

## [0.7.1] - 2026-10-22

### 重构
- 语音桥监督状态机（等链路/重试/失败等待/静默会话）从 Tauri 命令层下沉到
  `core-voice/src/supervisor.rs`，通过 `SupervisorHost` trait 与 UI 解耦，可脱离 Tauri 单测；
- `commands/audio.rs` 按职责拆分为 `voice_bridge`（启停与事件适配）、`audio_tools`（诊断/
  测试音/模拟/Win+H）、`vb_cable`（虚拟声卡检测与安装）；
- `core-voice` 新增端口抽象 `ports`（`VoiceTransport` / `AudioSink` / `KeySink`）及
  `transport` / `sink` / `keys` 真实实现，`bridge` 与 `simulate` 共用同一组接口；
- `core-voice/src/bridge.rs` 拆分为 `bridge/{mod,diag,handlers,pump,report}.rs`；
- 前端 `ConnectionPage` 逻辑抽到 `pages/connection/useConnectionActions.ts` /
  `useVbCableDriver.ts`，`App.css` 按页面拆分为 6 个页面样式文件（规则集合保持一致）；
- `core-dispatch/src/lib.rs` 拆分：动作执行线程移到 `executor.rs`、内联测试移到 `tests.rs`，
  触发判定与动作执行分离（lib.rs 1807 → 812 行，对外 API 不变）；
- `src-tauri/src/lib.rs` 拆分：路径 / 状态 / 按键兜底 / panic / 映射编解码移到 `support/`，
  lib.rs 只保留应用装配（683 → 377 行）；
- `core-voice/src/simulate.rs` 拆分为 `simulate/{mod,tts,wav,resample}.rs`：
  语音合成 / WAV 解析 / 重采样与链路编排各自独立；
- `src-tauri/src/commands/connection.rs` 拆分为 `connection/{status,device,prefs,system}.rs`：
  运行时状态 / 设备连接 / 用户偏好 / 系统集成四类命令分开；
- `src-tauri/src/commands/app_profile.rs` 拆分为 `app_profile/{menu,editor}.rs`：
  快捷菜单与窗口聚焦/启动 / 映射编辑器 CRUD 分开（360 → menu 150 + editor 210 行）；
- `core-hid/src/tap.rs` 拆分为 `tap/{config,hub,inject,runtime,status,watchdog}.rs`：
  配置与路径 / IPC 通信与分发 / 注入提权 / 启动生命周期 / 状态上报 / 看门狗与按住状态跟踪分开（tap.rs 1426 → 24 行）；
- `core-app-profile/src/lib.rs` 拆分为 `seeds.rs`（种子管理）、`store.rs`（磁盘原子读写）、`registry.rs`（内存匹配）、`tests.rs`（单元测试），lib.rs 885 → 63 行；
- `core-mapping/src/lib.rs` 拆分为 `combo.rs`（组合快捷键规范化与解析）、`config.rs`（映射配置结构与默认表）、`tests.rs`（单元测试），lib.rs 653 → 205 行；
- `core-input/src/vhid.rs` 拆分为 `vhid.rs`（驱动动态加载与报告提交，852 → 430 行）、`vhid/diag.rs`（Secure Boot / HVCI 探测与环境建议）、`vhid/tests.rs`；
- `core-ble/src/gatt.rs` 拆分出 `watcher.rs`（轻量链路监视器与快速连接检查，gatt.rs 667 → 601 行）；
- 前端 `MappingPage.tsx` 拆分：业务状态与应用配置/快捷键逻辑抽为 `useMappingEditor.ts`，遥控器尺寸动态自适应抽为 `useRemoteZoom.ts`（MappingPage.tsx 549 → 252 行）；
- 无用户可见行为变化。

### 文档
- TODO 移除「本地统计页」条目（保留 `core-stats` 后台统计能力）；
- AGENTS.md 规范「一个会话 = 一个分支 = 一个功能」，合并 / 发版必须经用户明确批准。

## [0.7.0] - 2026-10-22

### 新增
- **自动更新**：接入 Tauri v2 updater。正式构建启动后立即静默检查一次，运行期间每 30 分钟复查
  （自动检查距上次成功检查 ≥1 小时才联网，手动检查不受限）；也可在
  「通用设置 → 关于与更新」手动检查、查看更新说明、下载并安装；安装包使用独立的 minisign 密钥签名，
  下载后校验通过才会执行。NSIS 以 passive 模式安装，完成后自动重启应用；发现新版本时侧栏「通用设置」显示提示点。
  - 发布工作流新增：带签名的 updater 构建、`.exe.sig` 校验、`latest.json` 生成与上传；
    `latest.json` 的更新说明取自 CHANGELOG 对应版本小节。
  - 新增 `scripts/setup-updater-signing.ps1`（生成/轮换签名密钥并写入 GitHub Secrets）与
    `scripts/generate-updater-manifest.ps1`（本地可单独测试的清单生成脚本）。
  - 说明与限制见 `docs/项目/自动更新.md`。**旧版本（0.6.2 及更早）需手动安装一次带更新功能的版本**，
    之后才能走自动更新。

## [0.6.2] - 2026-10-20

### 变更
- 「断开」现在同时**暂停遥控器按键接管**：断开后遥控器按键不再触发映射、也不会再唤起豆包等输入法；
  点「连接」恢复语音与按键映射。连接页新增「语音桥已停止 / 按键接管已暂停」说明。
  （此前断开只停语音桥，HOGP 旁路 + 映射注入仍在工作，容易误解为「已断开却还能用按键」。）

### 修复
- 修复语音桥运行中「ATVV 端点」仍显示未就绪（橙色）的矛盾状态：端点就绪标志
  在会话状态快照之后才置位，快照里读到的是旧值。现在先置位再触发 running 回调、
  发现端点时即置位，前端再以「运行中必已就绪」兜底。


## [0.6.1] - 2026-10-20

### 修复
- 修复安装版无法「一键安装 VB-CABLE」的问题：安装包现在会把 VB-CABLE 下载/校验
  助手脚本（`install-vb-cable.ps1` + `vb-cable.sha256`）随包分发到
  `<安装目录>\scripts\`，应用从安装目录即可找到并运行。
- VB-CABLE 安装按钮改为「下载并安装 VB-CABLE」：明确会从 VB-Audio 官网下载、
  校验 SHA-256 后启动官方安装程序；提示需要管理员权限与**安装后重启电脑**。
- 安装助手不再假装静默安装（官方 Setup 是交互式的，会弹出自己的安装窗口）。

### 变更
- 引导页第 2 步更新：指向「诊断」页的下载安装入口，并补充重启要求。
- 遵守 VB-CABLE EULA：**不内置 VB-CABLE 官方包**，只随包分发我们自己的下载/校验脚本，
  由用户主动触发官方安装程序。


## [0.6.0] - 2026-10-20

### 新增
- **容错能力强化**：panic hook（写 `panic.log`）+ FFI 回调 `catch_unwind` + 锁中毒容错；
  WinRT 异步操作统一超时；CCCD/TX/服务打开状态校验；指数退避重连与 failed 状态；
  单实例互斥；音频严格设备匹配 + 队列封顶 + 音频流错误自动重建；配置 `config.json.bak`
  最后有效备份；HOGP 旁路 IPC 读超时；按键日志限流；虚拟键看门狗（断链/主线程卡死自动释放）。
- **启动自动连接**：记住上次设备，启动时自动连接并拉起语音桥（修掉 StrictMode 下永不触发的 bug）。
- **链路健康检测**：识别「链路已连接但连续零控制包」的遥控器语音固件挂起并给出可操作提示；
  诊断页新增「最近语音会话健康」自检项。
- **音频输出格式协商**：输出设备非 48k / 立体声 / f32 时自动重采样、声道映射、样本格式转换，
  并记录实际协商结果；修复用户改过 CABLE 格式后无声/变调的问题。
- **语音桥事件驱动会话恢复**：新增 `LinkWatcher` 监听 BLE 链路状态，去掉「每 30 秒无限盲重连」；
  连接页新增「等待遥控器上线 / 停止等待 / 重试连接」状态与按钮，失败只做有限重试。
- **状态一致性改造**：落实单一事实来源 + 派生状态（连接状态、语音桥状态、端点就绪、
  映射/触发时间以重读后端为准）；统一停止标志、输出设备名、ATVV 默认帧长常量。
- **映射目录单一事实来源**：新增 `get_mapping_catalog` 命令统一下发按键 / 触发方式 /
  动作分类 / 默认绑定，前端本地常量仅作浏览器预览兜底（含 5 个目录完整性测试）。

### 修复
- 修复遥控器断链后虚拟修饰键卡死（rctrl 卡在系统里、物理键盘失灵）。
- 修复「已连接 + 等待遥控器上线」同屏矛盾状态。
- 修复停止语音桥时误报「已断开」（实际仍在重连）。
- 修复自动连接在 React StrictMode 下不生效。
- 修复音频设备断连后输出流不恢复、播放静默回退默认设备的问题。
- 修复配置损坏时直接丢失配置（现自动回退最近一次有效备份）。

### 变更
- 语音桥重连语义：断链 → 等待系统重连（事件驱动）；仅「链路仍在但会话异常」才有限重试。
- WinRT 等待改为带超时轮询，避免线程永久挂起。
- 统计改为内存累计 + 定时批量落盘；前台应用查询增加 800ms 缓存。
- 文档：更新 `docs/项目/容错能力评估.md`（含状态一致性审计）、任务清单与协议待办。


## [0.5.0] - 2026-09-20

### 新增
- **应用专属按键配置**：按键映射可以按前台应用分别设置。命中配置的应用自动切到自己那一套，其余情况走全局映射；**麦克风键不受应用配置影响**，永远按全局来。
  - 新增 `core-app-profile` crate：按进程名（如 `Codex.exe`）与窗口标题匹配前台窗口。
  - 一个应用一个 JSON 文件，放在 `%LOCALAPPDATA%\RemoteMic\RC003\app-profiles\`，按文件名顺序加载。内置的 Codex / Claude Code Desktop / DeepSeek Harness / ZCode 只是**首次运行的种子**，铺到磁盘后就能改名、改匹配、删除——除「全局」外全部可删。
  - 按键映射页新增应用选择栏：新建 / 编辑 / 删除应用配置，并为每个应用单独绑定按键；左右两栏分别列出全局与该应用的绑定，重叠的以应用为准。
  - 长按判定与双击间隔从按键映射页搬到新的「通用设置」页。
- **快捷菜单内圈加入应用图标**：点一下——没开就启动，已开就切到前台。
- **自定义快捷键可命名**：动作分类「自定义」里可以新建、改名、删除快捷键，绑定时显示名字，不用记按键组合。
  - 名字只是显示层：绑定里存的仍然是 `combo:lctrl+k`，所以改名或删除都不影响已经绑好的按键，也不影响旧配置（`shortcuts` 字段缺省即为空）。
  - 支持只录一个修饰键（如右Ctrl），也支持单独的字母、数字、F1–F12、Enter/Tab 等。
- 新增「聚焦输入框」动作；「聚焦输入框 / 回车」作为独立动作保留——只有焦点不在输入框时才发回车，不覆盖「回车」本身的语义。

### 修复
- 修掉三处卡死：快捷菜单重入死锁（关闭快捷菜单卡死主线程）、点应用图标卡死主线程、调度器锁与前台查询互锁（AppHang 的真正原因）。
- 录制自定义快捷键时组合键一闪而退：
  - 先按主键再按修饰键时主键被丢弃（人手同时按下两个键时，操作系统给出的先后是随机的）。
  - 松开修饰键时会误清空 F 键与 Enter/Tab 这类按键——判断「有没有主键」错用了 token 长度，只有字母数字碰巧符合。
  - 现在按下顺序不再影响结果，松手也不会改动已录内容。
- 只录一个修饰键（如右Ctrl）不再被拦下：后端本就支持「修饰键可单独使用」。
- 修掉按键映射页在窄窗口下两列网格的横向溢出。

### 变更
- 按键映射页改版：铺满可视区不再出现整页滚动条、左右两栏等高、遥控器卡片去掉外壳与外阴影只留图形本身、动作分类合并（方向 / 音量播放 / 语音并入「系统」，「快捷键」改名「自定义」）、去掉 ①②③ 编号与「即将保存」预览块。
- 新增设计稿 `docs/design/app-profile-mapping.html`；`profiles/README.md` 说明应用配置的匹配规则与字段。

## [0.4.2] - 2026-09-13

### 修复
- 开发构建（`npm run tauri dev` / `cargo run`）下虚拟 HID 自检会误报「请用最新安装包重新安装 Remote Mic」，让人以为安装坏了。
  - 新增开发构建识别 `is_dev_build`（`<仓库>\target\{debug,release}\`）：这类构建既没有随包负载、也不会触发安装器的自动安装，现在提示改为「先运行 scripts/prepare-vhid-bundle.ps1 生成负载，或直接安装正式安装包」。
  - `VhidDiagnostics` 新增 `dev_build` 字段。

### 变更
- `scripts/prepare-vhid-bundle.ps1` 现在会顺带把负载铺到 `target\debug\vhid` 与 `target\release\vhid`（若存在），使开发构建也能找到 `WinUHid.dll` 并让「安装 / 修复虚拟键盘驱动」按钮可用。

## [0.4.1] - 2026-09-13

### 新增
- 虚拟 HID 自检升级为**环境诊断**：结论直接带出 Secure Boot 与内存完整性（HVCI）状态，失败时按具体情况给出可执行的下一步。
  - 新增 `core_input::vhid_diagnostics()`（`VhidDiagnostics`）：可用性、安装包负载目录及是否就位、Secure Boot / HVCI 状态、排查建议。
  - 建议由纯函数 `build_hints` 生成：负载缺失 → 用最新安装包重装；负载就位 → 点「安装 / 修复虚拟键盘驱动」；Secure Boot 或 HVCI 开启 → 点名说明测试签名可能被拒；任何失败都附 `setupapi.dev.log` 定位指引。
  - 新增 6 个单元测试，覆盖「开启才点名、关闭/未知不误报」等组合。

### 变更
- 诊断页自检结论按 `pre-line` 分行显示建议，不再挤成一行。
- 面向用户的错误文案不再提示手工运行 `build-winuhid.ps1` / `install-winuhid.ps1`（安装包已自动处理），并去掉 `Windows error:` 包装前缀。

## [0.4.0] - 2026-09-13

### 新增
- 安装包内置 Frida Gadget，安装后无需任何运行时下载：
  - HOGP 旁路依赖 `frida-gadget.dll`，此前由应用在首次需要时从 GitHub Releases 下载（6.8 MB），国内网络经常失败——而返回/音量/TV 键只有这条通道可见，实测麦克风键也走这条通道。
  - `gadget_fetch.rs` 改为优先使用安装包内置的压缩包：先查 `<安装目录>\vhid\<name>.dll.xz`，校验通过则复制到 `%PROGRAMDATA%\RemoteMic\hid-tap` 并解压；仅在缺失或校验失败时才回落下载。
  - `scripts/fetch-frida-gadget.ps1` 新增 `-DestDir` 与 `-ArchiveOnly`，供 `prepare-vhid-bundle.ps1` 把归档拉进安装包负载；Release 工作流校验负载中必须含 `*.dll.xz`。
  - 安装包体积 2.4 MB → 9.2 MB（Gadget 的 `.xz` 本身已压缩，几乎原样计入）。
  - 新增离线验证测试：用真实归档走完「内置 → 校验 → 解压」，确认不联网也能得到可用 DLL。

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
