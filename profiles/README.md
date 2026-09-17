# 应用专属按键配置（App Profiles）

**一个应用一个 JSON 文件。** 用于在某个应用处于前台时，覆盖全局按键映射。

由 `crates/core-app-profile` 加载与匹配，**不依赖**调度器 / 注入层 / Tauri，可单独测试。

---

## 怎么生效

```
前台窗口 → 进程名（如 Codex.exe）→ 匹配到一份 profile → 覆盖全局映射中对应的 (按键, 触发)
```

- 匹配用**可执行文件名**，大小写不敏感，传完整路径也只取文件名
- 进程名都不匹配时，再按**窗口标题**兜底匹配（`window_title_contains`）
- profile 里**没写**的 `(按键, 触发)` 继续沿用全局映射（`config.json` 的 `mapping`）

## 唯一的来源：磁盘上的文件

| 角色 | 位置 | 说明 |
| --- | --- | --- |
| **种子** | 本目录 `profiles/*.json` | 由 `build.rs` 编进二进制，只当模板用。**新增应用只要往这里放文件，不用改 Rust 代码** |
| **真身** | `<配置目录>/app-profiles/*.json` | 运行时的唯一事实来源。种子在**首次运行**时被写到这里，之后程序**不再覆盖它** |

`<配置目录>` 即 `%LOCALAPPDATA%\RemoteMic\RC003`。

```text
%LOCALAPPDATA%\RemoteMic\RC003\
├── app-profiles.seeded.json     ← 已落地清单，防止删掉的种子复活
└── app-profiles\
    ├── zcode.json               ← 首次运行从二进制落地
    ├── deepseek-harness.json
    └── 你新建的.json
```

**为什么不做「内置 + 用户」两层合并**：那样「删除一份应用配置」是删不掉的——
种子每次启动都从二进制里重新展开，删掉的那份下一轮自己就回来了。而界面上
「除了全局，其余都能删」这条规则，只有在配置真的躺在磁盘上时才成立。

代价是**种子内容在编译期冻结**：改了本目录某个 JSON，只对还没落地过该 id 的
机器生效。已经落地过的用户要自己删掉那个文件，或者删掉整个 `app-profiles/`
目录加清单文件来**恢复出厂**。

### 文件名就是身份

`id` 是文件名去掉 `.json`（`zcode.json` → `zcode`），它**不写在 JSON 里**，
保存和删除都靠它定位。`name`（展示名）只是给人看的，**可以随便改**——正因如此
它不能当身份用，否则改个名字，快捷菜单的「点图标」和编辑器的保存就同时找不到目标。

文件名一律是 ASCII（从进程名 slug 化，`ZCode.exe` → `zcode`；没有进程名时用
`app`、`app-1`……）。中文名放在 JSON 的 `name` 字段里。

---

## 文件格式

和 `config.json` 的 `mapping` **同构**，可以直接互相拷贝：

```json
{
  "process": ["Codex.exe", "ChatGPT.exe"],
  "name": "Codex 桌面版",
  "note": "这个 profile 适配的是哪个版本/形态，哪些键还没实测",
  "window_title_contains": ["ChatGPT", "Codex"],
  "icon": { "label": "Cx", "color": "#10A37F" },
  "launch": { "kind": "appid", "value": "dev.zcode.app" },
  "bindings": [
    { "button": "Up",   "trigger": "SingleClick", "action": { "KeyCombo": ["pageup"] } },
    { "button": "Ok",   "trigger": "SingleClick", "action": "Return" },
    { "button": "Back", "trigger": "SingleClick", "action": "Escape" }
  ]
}
```

### 字段

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `process` | | 进程名。可以写字符串，也可以写数组（同一应用的多个可执行文件）。**可以整条省略**，那就只按窗口标题匹配 |
| `name` | | 展示名。留空时 UI 退回显示进程名 |
| `note` | | 备注。**建议写清哪些快捷键还没实测** |
| `window_title_contains` | | 窗口标题关键字，命中任意一个即算匹配（大小写不敏感） |
| `icon` | | 快捷菜单内圈图标；**不填就不进菜单**（但前台自动切映射照常生效） |
| `launch` | | 「点图标」时怎么启动；不填则只能聚焦已打开的窗口 |
| `bindings` | | 覆盖项。`button` / `trigger` / `action` 见下 |

`process` 与 `window_title_contains` 至少要有一个，否则这份配置永远匹配不上。

### `window_title_contains`

进程名**认不出来**的目标才需要它：

- Chrome / Edge 里的 PWA 或网页应用（前台进程是 `chrome.exe`，区分不出是哪个站点）
- 跑在 WSL 里的服务：Windows 侧只有浏览器窗口，根本没有它的进程

进程名优先：只要有任意一份配置的 `process` 命中，就不再走标题匹配。
留空的标题串会被忽略，不会变成「匹配一切」。

### `icon`

```json
"icon": { "label": "DS", "color": "#4D6BFE" }
```

在快捷菜单内圈画一个「品牌色实心圆 + 白色字」的矢量简标（不依赖图标文件）。
`label` 建议 1–2 个字符，超过 2 个会自动缩小字号。应用已打开时图标下方会多一个小白点。

### `launch`

```json
"launch": { "kind": "path",  "value": "C:\\Apps\\Foo\\Foo.exe" }
"launch": { "kind": "appid", "value": "Claude_pzs8sxrjxfjjc!Claude" }
"launch": { "kind": "url",   "value": "http://127.0.0.1:3088" }
```

| `kind` | 适用 | 怎么查 |
| --- | --- | --- |
| `path` | 普通安装包 | 可执行文件完整路径 |
| `appid` | MSIX / 打包应用、注册过开始菜单的 Electron 应用 | `Get-StartApps` 输出的 `AppID` 列 |
| `url` | 网页应用、本地服务 | 直接写 URL |

`appid` 走 `shell:AppsFolder\`，**与本机安装路径无关**，换台机器也能用，
所以它是首选；`path` 最直白但只在那台机器上对。
`kind` 拼错会直接报错，不会静默退化成某个默认值。

点图标时的行为：**先找已打开的窗口并切到前台，找不到才启动**（顺序反了会开出第二个实例）。

### `button`（13 键）

`Power` `Up` `Down` `Left` `Right` `Ok` `Back` `Home` `Menu` `Tv` `VolumeUp` `VolumeDown` `Mic`

### `trigger`

`SingleClick` `DoubleClick` `LongPress` `Press` `Release`

### `action`

常用两种写法：

```json
"action": "Return"                              // 内置动作，直接写名字
"action": { "KeyCombo": ["lctrl", "k"] }        // 自定义快捷键
```

内置动作可选值（完整列表见 `core-mapping` 的 `ActionKind`）：

`Disabled` `Escape` `Return` `ArrowUp` `ArrowDown` `ArrowLeft` `ArrowRight`
`DeleteBackward` `ShowDesktop` `ContextMenu` `AppSwitcher`
`SystemVolumeUp` `SystemVolumeDown` `SystemVolumeMute` `PlayPause` `Voice`
`FocusInput` `FocusInputOrSubmit` `ToggleQuickMenu` `OpenApp("<名字>")`

> **`FocusInput`（聚焦输入框）**：用 UI Automation 把键盘焦点送进**前台窗口**的
> 输入框。切窗口只抬升顶层窗口，光标不会自己进输入框——想要「切到某个应用后
> 直接说话/打字」，就把它的确定键绑成这个动作。
> 只对向 UIA 暴露控件树的应用有效（Chromium / Electron 系可以），找不到输入框
> 时静默跳过。
>
> **`FocusInputOrSubmit`（聚焦输入框 / 回车）**：确定键的顺手版本——
> 光标不在输入框就送进去（接下来用户要说话），**已经在里面就发回车**；
> 连找不到输入框也退回普通回车。所以它是 `Return` 的**超集**，
> 绑它不会弄丢确定键原本的回车能力。推荐绑在确定键的单击上。
>
> **注意**：浏览器里第一个 `Edit` 往往是地址栏，给网页应用配这两个动作前先在
> 诊断页确认前台窗口，不要想当然。

快捷键 token 支持：`lctrl` `rctrl` `lshift` `rshift` `lalt` `ralt` `lwin` `rwin`、
`a`–`z`、`0`–`9`、`f1`–`f12`、`enter` `esc` `space` `tab` `backspace` `delete`
`insert` `home` `end` `pageup` `pagedown` `up` `down` `left` `right` `apps`。

> 左右修饰键是**分开**的：`lctrl` 和 `rctrl` 不是同一个键。

---

## 新增一个应用

### 走界面（推荐）

按键映射页 →「作用范围」→「+ 新建应用」。填展示名和进程名，「用当前前台填入」
按钮直接读当前前台窗口的进程名——**别去任务管理器里抄**，抄错一个字母这份配置
就不会命中，而且不会有任何报错。

建好后在这份配置下改按键，只有你改过的格子会覆盖全局。胶囊右上角的 **×**
删掉整份配置（先弹二次确认）；向导里的「改用全局」只撤掉选中的那一格。
两者不是一回事。

### 手写 JSON

1. 打开该应用，**在任务管理器里查它的可执行文件名**（`详细信息` 选项卡 → 名称列）。这一步别猜。
2. 在 `<配置目录>/app-profiles/` 新建 `<id>.json`（**文件名只能是 ASCII**），填 `process` 和 `name`。
3. 只写你**实测确认**过的快捷键；不确定的先不写（会自动沿用全局映射），并在 `note` 里记一句。
4. 想在快捷菜单内圈露脸，再加 `icon`（可选 `launch`）。
5. 重新加载配置（重启应用，或调用一次 `reload_app_profiles`）即可生效。

调试用：诊断页会显示**当前前台进程名 / 窗口标题**与**命中的 profile**，用它来核对第 1 步。

> **改本目录（仓库里的种子）要重新构建。** 种子是编进二进制的（`build.rs` +
> `include_str!`），`npm run tauri dev` 的文件监视只盯着 `crates/` 与 `src-tauri/`，
> **看不到仓库根的 `profiles/`**，所以改完要么重启 dev，要么碰一下
> `crates/core-app-profile/` 下的文件来触发重建。而且——**只对还没落地过该 id
> 的机器生效**，你自己这台已经有 `app-profiles/zcode.json` 了，得先删掉它。
>
> 改 `<配置目录>/app-profiles/` 里的文件不受此限，重新加载即可。

---

## 约定

- **不要覆盖 `Mic` 键。** 语音输入 / 按住说话是全局长按逻辑，跟具体应用无关；profile 覆盖它会让语音功能在该应用里失效。代码里也做了兜底，覆盖了也不生效。
- **不要凭猜测填快捷键。** 宁可先留空沿用全局映射，也不要写一个没验证过的组合——它会静默地做错事。
- 快捷键在不同平台常不一样（官方文档多以 ⌘ 描述），**Windows 版必须实测**。
- `note` 里如实记录未验证项，方便后续补齐。
- **`launch` 里的路径是本机事实，不是通用事实。** 写 `path` 时在 `note` 里说明，或优先用 `appid`。

---

## 已知限制

- **窗口标题匹配是全局兜底，可能误伤。** 它只在「没有任何配置的进程名命中」时才启用；
  如果标题关键字选得太泛（比如只写 `Claude`），浏览器里一个同名标签页也会命中。
  关键字要选得足够独特。
- **标题匹配需要窗口标题稳定。** 浏览器窗口标题会随标签页/会话变化，
  拿它做匹配前先确认关键字在目标站点里始终出现（DSH 的窗口标题结尾恒为 `DeepSeek Harness`）。
- **从后台进程抢前台会被系统拒绝。** 点快捷菜单时本应用就是前台，不受影响；
  但如果将来做「应用启动后自动切过去」，目标是刚启动的进程时可能只闪任务栏。
