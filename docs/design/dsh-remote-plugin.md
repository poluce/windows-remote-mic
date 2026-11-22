# 遥控器 × DSH：快捷键适配与「遥控器命令」插件设计

> 目标：用 RC003 遥控器的 13 个键，在不碰鼠标键盘的情况下完成 DSH（DeepSeek Harness）里
> 最常用的一整套操作——切换会话 / 切换项目 / 翻记录 / 发送 / 停止 / 开关面板。
>
> 本文先盘点「DSH 已经有什么」「遥控器能发什么」，再给出插件要补的命令清单与实施计划。
> 状态：**设计稿**（尚未实现）。

## 1. 为什么需要插件

DSH 的 Web 端已经有一批可自定义的快捷键（见 §3），遥控器只要能发出那些组合键就能用。
实测下来有三类缺口：

1. **会话之间来回切**：DSH 只有「搜索会话」（Ctrl+Alt+K 打开命令面板），没有
   「上一个会话 / 回到刚才那个会话」。
2. **按项目（Workspace）切换**：有「添加工作区」，没有「上一个 / 下一个项目」。
3. **焦点与消息级操作**：没有「聚焦输入框」「聊天区翻页」「停止当前回复」的默认快捷键。

这些缺口不是 Remote Mic 能补的（Remote Mic 只会发键），只能在 DSH 侧注册新命令，
因此做一个 **DSH 客户端插件**：只补缺口，不重写 DSH 已有的命令。

非目标：

- 不替换 DSH 现有快捷键，不改 DSH 本体；
- 不做 Host 侧（模型工具）能力，纯浏览器端命令；
- 不做「把 DSH 所有操作都注册一遍」——那会让键位、默认值与版本升级互相打架。

## 2. 遥控器能发什么

Remote Mic 的动作是「按键组合 → 注入键盘事件」，`core-mapping/src/combo.rs` 目前接受：

| 类别 | 可写键名 |
| --- | --- |
| 修饰键 | `lctrl` `rctrl` `lalt` `ralt` `lshift` `rshift` `lwin` `rwin` |
| 字母 / 数字 | `a`–`z`、`0`–`9` |
| 功能键 | `f1`–`f12` |
| 导航 | `up` `down` `left` `right` `home` `end` `pageup` `pagedown` |
| 编辑 | `enter` `esc` `tab` `space` `backspace` `delete` `insert` `apps` |
| 标点 | `/` `,` `.` `` ` `` `-` `=` `;` `'` `[` `]` `\`（或英文名 `slash` `comma` …） |

约束：

- 一次最多一个主键 + 若干修饰键，注入的是**真实键盘事件**；
- 目标应用必须先收到焦点（Remote Mic 的应用配置按**前台进程 / 窗口标题**匹配）；
- 组合键不能与系统级快捷键冲突（如 `Alt+Tab`、`Win+…` 会被系统先吃掉）。

## 3. DSH 现有快捷键（实测自 Web / Windows 默认值）

| 命令 | 默认键位 |
| --- | --- |
| 新建会话 | `Ctrl+Alt+N` |
| 搜索会话 | `Ctrl+Alt+K` |
| 添加工作区 | `Ctrl+Alt+O` |
| 重命名会话 | `Ctrl+Alt+G` |
| 分叉会话 | `Ctrl+Shift+F` |
| 归档会话 | `Ctrl+Alt+A` |
| 左侧栏开关 | `Ctrl+Alt+B` |
| 右侧栏开关 | `Ctrl+Shift+B` |
| 文件面板 | `Ctrl+Alt+P` |
| 新终端 | ``Ctrl+` `` |
| 新浏览器页 | `Ctrl+Alt+T` |
| 用本地应用打开 | `Ctrl+Shift+O` |
| 全屏 | `Ctrl+Alt+Enter` |
| 关闭页面 / 刷新页面 | `Ctrl+Alt+W` / `Ctrl+Alt+R` |
| 设置 | `Ctrl+Alt+,` |
| 快捷键速查 | `Ctrl+/` |

这些都可以直接在 Remote Mic 的应用配置里绑给遥控器，不需要插件。

## 4. 插件要补的命令

命名空间前缀 `remote.`，全部注册进 DSH 的快捷键服务，因此会**自动出现在「快捷键速查」里**，
也可以由用户在 DSH 设置里改键。

### P0（解决当前走不通的流程）

| 命令 id | 名称 | 语义 | 建议默认键 |
| --- | --- | --- | --- |
| `remote.session.back` | 回到上一个会话 | 按「访问历史栈」回到最近离开的那个会话 | `Ctrl+Alt+[` |
| `remote.session.next` | 下一个会话 | 在「最近活跃会话」列表中前进一格（可跨项目） | `Ctrl+Alt+]` |
| `remote.workspace.prev` | 上一个项目 | 切到工作区顺序里的上一个项目 | `Ctrl+Alt+Shift+[` |
| `remote.workspace.next` | 下一个项目 | 切到下一个项目 | `Ctrl+Alt+Shift+]` |

「访问历史栈」由插件自己维护：监听当前会话变化，把上一次的会话 id 压栈；
`session.back` 弹栈并打开。这样「输入框 → 去别的项目 → 再按一下回来」可以一键完成。

### P1（补齐遥控器操作手感）

| 命令 id | 名称 | 说明 |
| --- | --- | --- |
| `remote.composer.focus` | 聚焦输入框 | 遥控器在会话里滚动后，一键回到可输入状态 |
| `remote.messages.pageUp` / `pageDown` | 消息区翻页 | 若消息区不响应 PageUp/PageDown，用公开 API 滚动 |
| `remote.turn.stop` | 停止当前回复 | 对应界面上的「停止」按钮 |
| `remote.session.pinList` | 打开会话列表 | 复用 `session.search`，把面板当作列表用 |

### P2（可选）

- 在当前项目内循环会话；
- 跳到最新消息 / 最后一条用户消息；
- 会话内搜索（如果 DSH 提供公开的查找 API）。

## 5. 遥控器键位方案（在插件就绪后）

| 遥控器键 | 单击 | 长按 |
| --- | --- | --- |
| 上 / 下 | `↑` / `↓`（列表与面板选择） | `PageUp` / `PageDown`（聊天区翻页） |
| 左 / 右 | 左侧栏 / 右侧栏开关 | — |
| 确定 | `Enter` | — |
| 返回 | `Esc` | **回到上一个会话**（`remote.session.back`） |
| 主页 | 新建会话 | 上一个项目（`remote.workspace.prev`） |
| 菜单 | 快捷键速查 | 打开设置 |
| TV | 搜索会话 | 下一个项目（`remote.workspace.next`） |
| 音量 ± | 系统音量 | — |
| 电源 | 不绑定（避免误关） | — |
| 麦克风 | 按住说话（全局，不走应用配置） | — |

## 6. 技术方案

### 6.1 形态

- **只做 Client 半**：`package.json` 里 `dsh.client = { platform: "web", immediately: true, inject: [...] }`，
  `client.js` 通过 `window.__ModuleLoader__.load({ id, factory })` 注册插件；
- **bundle 补丁**插入一条 Host 空壳行（`index.js` 只 `export function apply() {}`），
  这样它和普通插件一样出现在「插件」页，可随时禁用 / 卸载；
- 不新增依赖，不碰 Host 服务。

### 6.2 依赖的客户端服务

| 用途 | 服务 / API | 状态 |
| --- | --- | --- |
| 注册命令 | `ctx.shortcuts.register({ id, label, aliases, defaults, regions, modals, resolve })` | 已确认 |
| 会话 / 项目列表 | `ctx.workspaces` 快照（`workspaces.list`，含 workspace 分组与 session 摘要） | 待确认公开形态 |
| 打开指定会话 / 项目 | `uiWorkspace.openSession(id)` / `openWorkspace(id)` | **需先验证是否对外暴露** |
| 本地化 | `ctx.locale`（命令名走 i18n） | 已确认 |
| 停止回复 / 输入聚焦 | 会话与输入框相关服务 | 待确认 |

> 关键风险：**「打开指定会话」目前只在 `ui-workspace` 包内部使用**。若没有公开的
> navigation 服务，需要先走一遍 API 发现（`ctx` 上的服务名、`dsh --profile web --dump-config`
> 与安装包源码），确认后再实现；实在没有公开入口时，退化为「只提供 `session.back` 需要的
> 历史栈 + 调用 `session.search` 打开面板」并记录限制。

### 6.3 命令的默认键位与冲突

- 每条命令必须为 `web:windows` / `web:macos` / `web:linux` / 桌面端声明默认值，缺失即未绑定；
- 快捷键服务会拒绝与现有默认值重叠的组合；实现前先跑一遍「默认键位冲突检查」，
  因此上表的 `Ctrl+Alt+[` / `Ctrl+Alt+]` 需要确认未被占用；
- 用户改键后覆盖存 `dsh.keybindings.v1`（Web）或 `userData/keybindings.json`（桌面），
  插件卸载后覆盖仍保留。

### 6.4 行为细节

- `session.back` 的历史栈只记录「成功切走的会话」，上限 20 条，去重；
- 新会话（空白页）不压栈；
- 目标会话已被归档 / 删除时，跳过该条并提示；
- 命令在会话搜索面板打开时不可用（`modals` 限制交给服务判断）。

## 7. 实施步骤

1. **API 发现**（0.5 天）：确认会话列表、打开会话、停止回复、输入聚焦的公开入口；
2. **插件骨架**（0.5 天）：bundle + client 入口 + 一条 `remote.ping` 命令，验证它出现在速查里；
3. **P0 命令**（1 天）：`session.back/next`、`workspace.prev/next` + 历史栈；
4. **Remote Mic 配置**（0.5 天）：更新 `profiles/deepseek-harness.json` 的返回 / 主页 / TV 长按，
   同时写一份「用户机器上的迁移说明」；
5. **联调**（0.5 天）：真机按一遍 §5 的键位表，记录哪些组合被系统/浏览器吃掉；
6. **文档**：本文更新为「已实现」，补插件 ID、安装方式、已知限制。

## 8. 验收标准

用遥控器完成下面这条链路，全程不碰键盘鼠标：

1. 在会话 A 的输入框里（焦点在输入框）；
2. 按 TV 打开会话搜索 → 上下选到项目 B 的会话 → 确定进入；
3. 按返回长按 → **直接回到会话 A**；
4. 按主页新建会话；
5. 按主页长按 / TV 长按在项目间前后切换；
6. 按菜单长按打开设置，返回关闭。

任一步失败都记录：当前前台窗口、Remote Mic 日志里的动作、DSH 是否收到按键。

## 9. 风险与待验证

| 风险 | 影响 | 处理 |
| --- | --- | --- |
| 无公开的「打开指定会话」API | `session.back` 无法实现 | 先做 API 发现；退化为搜索面板方案并明确告知 |
| 默认键位与 DSH 现有快捷键冲突 | 命令注册失败 | 注册前做冲突检查，改用 `Ctrl+Alt+F7/F8` 等空闲键 |
| 浏览器 / 系统吃掉某些组合 | 按键无效 | 联调时逐项记录，必要时改用 `Alt+…` |
| DSH 版本升级改默认键位 | 遥控器配置失效 | 插件只依赖自己的 `remote.*` 命令；更新后重跑冲突检查 |
| 遥控器配置指向开发目录 | 换安装版后失效 | 文档里写明 sidecar / 配置路径的迁移步骤 |

## 10. Remote Mic 侧需要同步的改动

- `profiles/deepseek-harness.json`：加入返回 / 主页 / TV 的长按绑定（依赖插件命令的默认键位）；
- `docs/项目/` 增加「遥控器操作 DSH」的使用说明（键位表 + 插件安装）；
- 标点组合键支持已完成（`Ctrl+[` / `Ctrl+]` 可用），无需再改 Remote Mic 代码。

## 11. 交付物

| 交付物 | 位置 |
| --- | --- |
| DSH 插件包 | `dsh-bundles/dsh-remote-keys/`（本地 bundle，`plugin_manager install_bundle` 安装） |
| 本文档（实现后更新） | `docs/design/dsh-remote-plugin.md` |
| Remote Mic 种子配置 | `profiles/deepseek-harness.json` |
| 使用说明 | `docs/项目/遥控器操作DSH.md`（待写） |
