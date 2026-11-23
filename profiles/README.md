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

```
%LOCALAPPDATA%\RemoteMic\RC003\app-profiles\
├── 你的应用.json
└── ...
```

- 目录里有什么就用什么，**没有内置配置**；
- 在应用内「按键映射 → + 新建应用」建配置最省事：会带到场进程候选、把你当前的全局按键表复制进来当起点；
- 直接往目录里放 JSON 也行，重启应用后生效。

## 仓库里不再内置种子模板

早期版本会往这个目录里播种 Codex / Claude Code Desktop / DeepSeek Harness / ZCode 四份模板，
**现在取消了**：应用键位属于用户自己的环境，不该由设备仓库替用户假设。

原因：

- 每个应用的快捷键都可能被用户改过，播种出来的绑定很容易失效，还很难被发现；
- 部分应用的绑定依赖第三方插件（例如 DSH 的空间导航插件），没装插件的人拿到一份没有用的配置；
- 种子只在首次启动落地一次，改仓库里的模板对老用户无效，反而制造「改了却没生效」的困惑。

如果你在本地构建时想预置自己的配置（仅供自用，不随官方发行版分发），
把 JSON 放进本目录即可——`crates/core-app-profile/build.rs` 会在编译期把 `profiles/*.json`
编进二进制，首次启动时铺到用户目录（已存在的同名文件不会被覆盖）。

## 文件格式

一个最小示例：

```json
{
  "process": "Codex.exe",
  "name": "Codex 桌面版",
  "window_title_contains": ["Codex"],
  "bindings": [
    { "button": "Back", "trigger": "SingleClick", "action": "Escape", "enabled": true },
    { "button": "Home", "trigger": "SingleClick", "action": { "KeyCombo": ["lctrl", "n"] }, "enabled": true }
  ]
}
```

- `process`：进程可执行文件名，可写单个字符串或数组；
- `window_title_contains`：标题兜底匹配（可选）；
- `bindings`：`button`（Up/Down/Left/Right/Ok/Back/Home/Menu/Tv/Power/VolumeUp/VolumeDown/Mic）、
  `trigger`（SingleClick/DoubleClick/LongPress/Press/Release）、
  `action`（语义动作字符串，或 `{ "KeyCombo": [...] }` 组合键）；
- `launch`（可选）：`{ "kind": "appid" | "path", "value": "..." }`，用于「点图标唤起」。
