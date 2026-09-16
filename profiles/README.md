# 应用专属按键配置（App Profiles）

**一个应用一个 JSON 文件。** 用于在某个应用处于前台时，覆盖全局按键映射。

由 `crates/core-app-profile` 加载与匹配，**不依赖**调度器 / 注入层 / Tauri，可单独测试。

---

## 怎么生效

```
前台窗口 → 进程名（如 Codex.exe）→ 匹配到一份 profile → 覆盖全局映射中对应的 (按键, 触发)
```

- 匹配用**可执行文件名**，大小写不敏感，传完整路径也只取文件名
- profile 里**没写**的 `(按键, 触发)` 继续沿用全局映射（`config.json` 的 `mapping`）

## 两个来源

| 来源 | 位置 | 说明 |
| --- | --- | --- |
| **内置** | 本目录 `profiles/*.json` | 由 `build.rs` 编进二进制。**新增应用只要往这里放文件，不用改 Rust 代码** |
| **用户** | `<配置目录>/app-profiles/*.json` | 覆盖内置：与内置配置**有任一进程名重叠**时整份替换 |

`<配置目录>` 即 `%LOCALAPPDATA%\RemoteMic\RC003`。

---

## 文件格式

和 `config.json` 的 `mapping` **同构**，可以直接互相拷贝：

```json
{
  "process": ["Codex.exe", "ChatGPT.exe"],
  "name": "Codex 桌面版",
  "note": "这个 profile 适配的是哪个版本/形态，哪些键还没实测",
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
| `process` | ✅ | 进程名。可以写字符串，也可以写数组（同一应用的多个可执行文件） |
| `name` | | 展示名。留空时 UI 退回显示进程名 |
| `note` | | 备注。**建议写清哪些快捷键还没实测** |
| `bindings` | | 覆盖项。`button` / `trigger` / `action` 见下 |

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
`ToggleQuickMenu` `OpenApp("<名字>")`

快捷键 token 支持：`lctrl` `rctrl` `lshift` `rshift` `lalt` `ralt` `lwin` `rwin`、
`a`–`z`、`0`–`9`、`f1`–`f12`、`enter` `esc` `space` `tab` `backspace` `delete`
`insert` `home` `end` `pageup` `pagedown` `up` `down` `left` `right` `apps`。

> 左右修饰键是**分开**的：`lctrl` 和 `rctrl` 不是同一个键。

---

## 新增一个应用

1. 打开该应用，**在任务管理器里查它的可执行文件名**（`详细信息` 选项卡 → 名称列）。这一步别猜。
2. 在本目录新建 `<应用名>.json`，填 `process` 和 `name`。
3. 只写你**实测确认**过的快捷键；不确定的先不写（会自动沿用全局映射），并在 `note` 里记一句。
4. 重新构建即可生效（`build.rs` 自动发现，不用改代码）。

调试用：应用内诊断页会显示**当前前台进程名**与**命中的 profile**，用它来核对第 1 步。

---

## 约定

- **不要覆盖 `Mic` 键。** 语音输入 / 按住说话是全局长按逻辑，跟具体应用无关；profile 覆盖它会让语音功能在该应用里失效。
- **不要凭猜测填快捷键。** 宁可先留空沿用全局映射，也不要写一个没验证过的组合——它会静默地做错事。
- 快捷键在不同平台常不一样（官方文档多以 ⌘ 描述），**Windows 版必须实测**。
- `note` 里如实记录未验证项，方便后续补齐。

---

## 已知限制

- **浏览器里的 Web 应用匹配不到。** 例如 DSH 的 Web GUI，前台进程是浏览器（`msedge.exe` / `chrome.exe`），按进程名无法区分是哪个站点。要支持这类目标，需要再加「窗口标题匹配」——目前**未实现**，等确有需要再做。
