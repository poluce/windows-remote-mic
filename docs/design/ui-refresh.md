# Remote Mic · 全应用 UI 统一与轻毛玻璃（设计说明 v1）

> **配套可视稿**：[ui-refresh.html](ui-refresh.html) —— 浏览器直接打开，含真实窗口 1000×700 的排版稿、token、组件库与整改清单。
> **两张预览图**（由可视稿无头渲染导出）：[连接页](ui-refresh-connection.png) · [按键映射页](ui-refresh-mapping.png)
> **分支**：`feat/ui-beautify`（基线 `main @ 52a072e`，v0.7.2）
> **状态**：设计稿，待评审。**本提交未改动任何产品代码**，只有 `docs/design/` 两个文件。

---

## 实现状态（2026-10-23）

| 阶段 | 状态 | 说明 |
| --- | --- | --- |
| 1. Token 底座 | ✅ 已完成 | 新增 `src/styles/tokens.css`（`--rm-*` + 过渡期旧变量别名，顺带修好 `--muted` / `--card`）；`App.css` 改为只引用 token |
| 2. 外壳 | ✅ 已完成 | 雾面底（柔光斑 + 噪点）、玻璃侧栏（`backdrop-filter`，不支持时退化实色）、单色导航、统一页头、细滚动条、统一焦点环 |
| 3. 控件统一 | ✅ 已完成 | 胶囊合并为 `.pill`；按钮加 `.on`（修启用态）；提示条改 `.callout`；删除 `.badge` / `.device-status-pill` / `.vbcable-status` / `.eat-*` / `.sim-*` 等重复与死样式 |
| 4. 页面精修 | ✅ 基本完成 | 六页均已套用；映射页选中态改淡底、档案圆点降饱和、遥控器高亮统一；日志/引导/设置/诊断色值对齐 |
| 5. 周边一致 | ✅ 已完成（深色模式除外） | `public/quick-menu.html`：强调色、文字色、以及画布里的应用品牌色都做了降饱和（`muteBrandColor`），已开窗验证；深色模式未做 |

**验证方式**：
- `npm run typecheck`、`npm run build` 通过；
- 先以 Vite dev server + 无头 Chromium（与 WebView2 同内核）在 1000×700、780×560 两种尺寸逐页核对；
- **真机验证已完成**：本分支 `cargo build` 后启动 `target/debug/remote-mic.exe`（Tauri 窗口、WebView2），遥控器在线、语音桥运行中，逐页截图并打开过快捷菜单窗口。截图在 `docs/项目/截图/全局外观-*.png`。

界面截图（真机 WebView2，1000×700）：[连接页](../项目/截图/全局外观-连接页.png) · [按键映射页](../项目/截图/全局外观-按键映射页.png) · [通用设置页](../项目/截图/全局外观-通用设置页.png) · [诊断页](../项目/截图/全局外观-诊断页.png) · [日志页](../项目/截图/全局外观-日志页.png) · [引导页](../项目/截图/全局外观-引导页.png) · [按键测试](../项目/截图/全局外观-按键测试.png) · [快捷菜单](../项目/截图/全局外观-快捷菜单.png)

> `780×560`（窗口最小尺寸）下按键映射页的矩阵会裁切——这是改造前就存在的布局限制，不是本次回归；默认 1000×700 下正常。

### 色板 v2（2026-10-24）：整体再提一点饱和度

v1 的低饱和色板被反馈「不够鲜艳」，于是在不回到原始高饱和的前提下整体提了一档（强调色 HSL 饱和度 34% → 48%，语义色 +25% 左右）：

| token | v1 | v2 | 白底对比度 |
| --- | --- | --- | --- |
| `--rm-accent` | `#44678a` | `#3a6ea5` | 5.3 ✅ |
| `--rm-ok` | `#4c7a5d` | `#3f7f55` | 4.8 ✅ |
| `--rm-warn` | `#8a6a33` | `#8f6c2e` | 4.8 ✅ |
| `--rm-err` | `#9c4a42` | `#a84036` | 6.1 ✅ |

同时雾面底色斑提高约 20% 不透明度（整体观感更「有颜色」而不是加亮控件），快捷菜单品牌色混合比例从 40% 降到 30%。截图已按 v2 更新。

### 实现时与设计稿的偏差（有意为之）

1. **侧栏 232px、内容列 936px**：按现有布局微调，未做 880 的硬切换（映射页需要更宽）。
2. **映射页页头压缩**：这一页必须一屏放下，页头比其它页少 6px 下边距；`780×560` 下矩阵本来就会裁切（改造前同样），未新增回归。
3. **提示条暂不带图标**：`.callout` 支持 `.callout-ico`，但现有文案里加图标会让三处提示块高度增加，先留结构不加节点。
4. **`.status-dot` 保留**：仍用于下拉触发器和语音桥状态行；语义与 `.pill` 区分（点 vs 胶囊）。

## 0. 一句话

把现在并存的「三套色板 + 六套按钮 + 十档字号」收敛成**一套冷灰色调 + 单一强调色**；浮层（侧栏 / 对话框 / 下拉 / 快捷菜单）用**真毛玻璃**，内容卡片用**磨砂纸**；不改信息结构、不改窗口配置、不改 Rust。

---

## 1. 结论摘要

### 1.1 要做的

| # | 改动 | 为什么 |
| --- | --- | --- |
| 1 | 建立 `--rm-*` 设计 token（约 50 个），`App.css` / 页面 CSS 全部改为引用 | 现在同一语义有 2–4 个值（4 套蓝、4 套绿、2 套灰阶），改一处颜色要翻 8 个文件 |
| 2 | 侧栏 6 个彩色图标块 → **单色线性图标 + 选中变蓝** | 6 个饱和色块是全应用最抢眼的东西，与「色调一致」直接冲突 |
| 3 | 背景改为「雾面底」：冷灰渐变 + 三团柔光色斑 + 1.5% 噪点 | 玻璃需要有东西可透；纯灰底上做玻璃等于没有玻璃 |
| 4 | 侧栏 / 对话框 / 下拉改为真 `backdrop-filter`；内容卡只用半透明磨砂 | 真模糊同屏 ≤3 层，避免掉帧与文字发虚 |
| 5 | 统一控件：`.btn` / `.pill` / `.switch` / `.seg` / `.input` / `.callout` 六类，删掉 5 套私有变体 | 现在同一次级按钮有 5 套配方，选中反馈也不同 |
| 6 | 统一页头（20px 标题 + 一行说明）、统一内容列宽 880、细滚动条 | 现在连接页自己 720 宽、其它页自然宽，切页会跳 |
| 7 | 修掉 2 个未定义 CSS 变量（`--muted` / `--card`）与小字对比度不达标 | 现存视觉 bug 与 a11y 缺陷 |
| 8 | 顺手清理约 60 行死样式、`.category-tabs` 重复定义、`transition: all` | 降低后续改样式的误伤面 |

### 1.2 明确不做的

| 不做 | 原因 |
| --- | --- |
| 系统 Mica / Acrylic（窗口透出桌面） | 微软 WebView2 团队确认：**窗口背景 alpha=0 时 `backdrop-filter` 没有背后像素可模糊，等于失效**（Chromium 设计）。所以「Mica 底 + CSS 磨砂卡」物理上做不到；而 Mica 仅 Win11 可用，Win10 直接无效，还要背透明窗口的白闪 / 文字发虚 / resize 掉帧 |
| 无边框自绘标题栏 | `titleBarStyle` 只对 macOS 有效；Windows 上丢 Snap Layouts（tauri#4531 仍 open）、拖边 resize 掉帧（#15569）、Win10 圆角/白边不一致 |
| 深色模式 | 本轮先做浅色；token 已按语义命名，后续加 `[data-theme="dark"]` 覆盖一层即可 |
| 布局/信息结构改版 | 只换材质、控件与层级；`MappingPage.css` 里那套 flex/grid 压缩规则一行不动 |
| 快捷菜单窗口结构改动 | `public/quick-menu.html` 是独立文档 + 内联样式，与新外壳天然隔离；本轮只对齐色值 |

### 1.3 需要你拍板（详见 §10）

1. 强调色锁定 `#007aff`？（现状三套蓝）
2. 侧栏图标改单色？还是保留六色（则需要 6 个 `--rm-nav-*`）
3. 日志页要不要按级别上色（需要日志页把纯文本切 span，约 30 行 TSX）
4. 是否接受「不做 Mica」

---

## 2. 现状诊断（证据）

### 2.1 三套色板并存

| 体系 | 出处 | 文字 | 强调灰/蓝 | 状态色 |
| --- | --- | --- | --- | --- |
| Apple 系（全局） | `src/App.css:1-34` | `#1d1d1f / #6e6e73 / #86868b` | `#007aff` | ok `#248a3d`、warn `#b25000`、err `#d70015` |
| GitHub 系（按键测试） | `src/components/RemoteKeyTester.css` | `#1f2328 / #57606a / #8c959f` | `#2f6fed` | ok `#22c55e`、淡底 `#f0fdf4` |
| Tailwind slate（快捷菜单 / 图标） | `public/quick-menu.html:214-323`、`public/app-icon.svg` | `#0f172a / #475569 / #64748b / #94a3b8` | `#007aff` / 图标 sky `#0EA5E9` | — |
| 写实美术（遥控器，豁免） | `src/components/Xiaomi2ProRemote.css` | 灰黑渐变 | 高亮 `#2f6fed` | done `#22c55e` |

同语义多值的具体数量：

| 语义 | 个数 | 值 |
| --- | --- | --- |
| 强调蓝 | 2 主 + 1 rgba 族 | `#007aff` / `#2f6fed` / `rgba(47,111,237,.7~.95)` |
| 成功绿 | 4 | `#248a3d` / `#34c759` / `#2da44e` / `#22c55e`（+ `#86efac`、`#f0fdf4`） |
| 警告 | 3 | `#b25000`（棕）/ `#d4a72c`（芥末）/ `#ff9f0a`（橙） |
| 危险红 | 3 | `#d70015` / `#d1242f` / `#ff4d4f`（+ hover `#b80012`） |
| 文字一/二/三级 | 各 2 | Apple ↔ GitHub 两套 |
| 次级表面 | 2 | `#f7f7f9` vs `#f6f8fa` |
| 统计点空态 | 4 个选择器同值 | `#cbd3dd`（`.status-dot` / `.off` / `.loading` / `.preview`） |

**结论**：不是「配色不好看」，是「没有唯一事实来源」。token 化的第一目标是**消重**，不是换色。

### 2.2 两个未定义的 CSS 变量（现存 bug）

| 变量 | 引用 | 后果 |
| --- | --- | --- |
| `--card` | `App.css:357`（`.ui-crash-card{background:var(--card)}`） | 计算值无效 → 背景透明；崩溃卡片与页面同色，**卡片外观消失** |
| `--muted` | `App.css:373`（`.ui-crash-hint`）、`ConnectionPage.css:166`（`.bridge-hint-body`） | `color` 失效 → 继承正文黑；提示正文与标题同级，**层级消失** |

另有 `RemoteKeyTester.css` 7 处 `var(--x, 硬编码)` 的 fallback（`#2f6fed/#1f2328/#57606a/#8c959f/#e3e7ec/#f6f8fa/#fff`），当前永不生效，但一旦改 CSS 加载顺序，诊断页会整页变 GitHub 风。**token 化时必须删掉 fallback，不能只换主值。**

### 2.3 次要文字对比度不达标（a11y）

实测（sRGB 相对亮度）：

| 前景 | 背景 | 对比度 | 结论 |
| --- | --- | --- | --- |
| `--text-2 #6e6e73` | `#f5f5f7` | 4.66:1 | 刚过 AA，玻璃上会跌破 |
| `--text-3 #86868b` | `#f5f5f7` | **3.33:1** | **违反 AA**，而 12px hint / 分区标签大量使用它 |

新 token：`--rm-text-2: #475569`（6.7:1）、`--rm-text-3: #5d6b7e`（白底 5.3:1 / 页底 4.8:1）；`#94a3b8` 只做图标与分隔线，不承载文字。

### 2.4 控件分裂

同一「次级按钮」的 5 套配方：`.btn`（7px / `4px 12px` / hover 变底）、`.trigger-btn`（8px / `6px 6px` / hover 变边）、`.scope-pill`（999px / `6px 14px` / 弱边框）、`.shortcut-btn`（7px / `4px 9px` / 选中反白）、`.status-select-trigger`（7px / `8px 12px`）。还有 `.mode-tab`（死样式）、`.log-refresh-btn`（圆形）。

「胶囊」5 套：`.badge`、`.vbcable-status`、`.device-status-pill`、`.scope-pill`、`.status-dot` + `.pill-dot`（6×6 规则在两处重复定义，圆点尺寸有 7/8/8/6/6 五档）。

焦点反馈 3 套：全局 `outline`（`App.css:52-57`）、输入类 `border+ring`、两处手写重复 outline；同时 `.trigger-btn.active` / `.scope-pill.active` 自带强调边框又会叠全局 outline，出现双强调环。

### 2.5 几何与字号散乱

| 项 | 现值统计 | 目标 |
| --- | --- | --- |
| `border-radius` | 18 种（4/6/7/8/9/10/12/14/999/50%） | **6 档**：6 / 8 / 12 / 16 / 20 / 999 |
| `font-size` | 13 种（10 / 10.5 / 11 / 11.5 / 12 / 12.5 / 13 / 13.5 / 14 / 14.5 / 15 / 18 / 20） | **6 档**：11 / 12 / 13 / 14 / 16 / 20 |
| `gap` | 17 种，1–14px 每个整数都出现过 | **5 档**：4 / 8 / 12 / 16 / 20（24/32 备用） |
| `box-shadow` | 7 套语义、14 类值，含纯黑 alpha | **3 级冷色阴影** + 1 个按钮辉光 |
| 边框 | 1px 为主，另有 0.5px（`.form-row`、`.form-foot`） | 统一 1px hairline |
| 过渡 | 0.12s / 0.15s / 0.18s / 0.2s 混用，2 处 `transition: all` | `180ms` + `ease-out`，展开具体属性 |

### 2.6 结构性

- **加载顺序陷阱**：`App.tsx:11` 在页面 import **之后**才引入 `App.css` → 全局规则排在页面 CSS 之后，同特异度时全局胜出。现存实例：`.matrix-hint{margin:0 0 8px}` 被 `.hint{margin:10px 0 0}` 盖掉，上边距实际是 10px。规范：页面要覆盖全局**必须提特异性或用变量**，不能靠顺序。
- **`.btn.active` 不存在**：`RemoteKeyTester.tsx:432` 写了 `btn ${active ? "active" : "primary"}`，但 `App.css` 没有 `.btn.active` → 启用状态反而退化成普通次级按钮，看起来「没选中」。
- **浅色下不可见的内联残留**：`RemoteKeyTester.tsx:450` 内联 `background: rgba(255,255,255,0.06)`（暗色时代留下的）。
- **死样式约 60 行**：`.sim-*`、`.eat-*`、`.mode-tab`、`.guided-*`、`.mapping-row` 系列在 TSX 已无对应结构；`.category-tabs` 定义两次；`.status-dot.off/.loading/.preview` 三条规则完全相同。
- **z-index 无体系**：10（下拉）/ 50（遮罩）/ 1（sticky 表头）/ 0–5（遥控器内部）。当前不出错，新增弹层就容易压过对话框。

---

## 3. 设计语言

### 3.1 五条原则（冲突时按编号优先）

1. **一个强调色**：只保留一种蓝，其余颜色只表达语义；颜色不再承担「区分页面」。
2. **玻璃只给浮层**：侧栏、对话框、下拉、快捷菜单用真模糊；内容卡片用半透明磨砂。
3. **冷灰四级文字**：与图标 / 快捷菜单已有的 slate 系对齐；文字只有四级。
4. **几何成阶梯**：圆角、阴影、字号、间距只允许取 token 里的档位。
5. **动效克制**：180ms、ease-out、只做状态反馈；尊重 `prefers-reduced-motion`。

### 3.2 核心色板（低饱和）

| token | 值 | 用途 | 对比度（白底） |
| --- | --- | --- | --- |
| `--rm-accent` | `#3a6ea5` | 主按钮、选中、图标 | 5.3 ✅ |
| `--rm-accent-strong` | `#2a5378` | 强调色小字、图标 | 8.1 ✅ |
| `--rm-accent-soft` | `rgba(58,110,165,.12)` | 选中底、hover 底 | — |
| `--rm-text` | `#131a24` | 正文、标题 | 17.5 ✅ |
| `--rm-text-2` | `#46536a` | 次级文字 | 7.8 ✅ |
| `--rm-text-3` | `#5c6979` | 提示、分区标签 | 5.6 ✅（页底 5.0） |
| `--rm-text-4` | `#93a0af` | 仅图标/分隔，不承载文字 | — |
| `--rm-ok` / `-fill` / `-soft` | `#3f7f55` / `#62a377` / `rgba(63,127,85,.13)` | 成功 文字 / 填充 / 底 | 4.8 ✅ |
| `--rm-warn` / `-fill` / `-soft` | `#8f6c2e` / `#c8a253` / `rgba(143,108,46,.13)` | 警告 | 4.8 ✅ |
| `--rm-err` / `-fill` / `-soft` | `#a84036` / `#c96e62` / `rgba(168,64,54,.11)` | 危险 | 6.1 ✅ |
| `--rm-line` / `-strong` / `-light` | `rgba(19,26,36,.08 / .16)`、`rgba(255,255,255,.72)` | 描边 / 玻璃上沿高光 | — |

### 3.2.1 色彩克制规则（按键 / 选中态）

| 场景 | 不要 | 改为 |
| --- | --- | --- |
| 选中的动作 / 触发方式 chip | 整块高饱和蓝底 + 白字 | 10% 淡底 + 强调色深字 + 1px 同色描边 |
| 矩阵「本应用覆盖」格 | 强调蓝铺满 | 7% 淡底 + 深色字 |
| 应用档案圆点（Claude / Codex / ZCode） | 后端色原样（`#D97757` / `#4D6BFE` / `#F59E0B`）跳色 | CSS `filter: saturate(.55)` + 1px 深色细环（数据不改） |
| 遥控器高亮 | `#2f6fed` 亮蓝 + 蓝色辉光 | `--rm-accent`；引导态才保留呼吸动画 |
| 状态点 / 胶囊 | iOS 亮绿 / 亮橙 / 亮红 | 灰绿 `#62a377` / 沙金 `#c8a253` / 陶土红 `#c96e62`，文字深一档 |
| 主按钮 | 亮蓝渐变 + 强辉光 | 钢蓝渐变 `#4A76AB→#345C8A` + 极轻投影 |

判断标准：把截图缩到 25% 看整体——第一个跳出来的色块若不是主按钮，就是过饱和。

**为什么是钢蓝而不是原亮蓝**：`#007aff` 这类高明度蓝在灰底上会「喊」，一屏里出现 3 次就显得廉价；`#44678a` 压得太灰，最终取中间值 `#3a6ea5`（白字对比度 5.3:1），主按钮与选中态都稳，也不至于闷。快捷菜单与遥控器高亮统一到它。

### 3.3 三级材质

| 层 | 配方 | 用在哪 |
| --- | --- | --- |
| **浮层玻璃** | 白 62% + `backdrop-filter: blur(24px) saturate(180%)` + 1px `rgba(255,255,255,.72)` 高光边 + `0 6px 18px -8px rgba(15,23,42,.18)` | 侧栏、对话框、下拉菜单、快捷菜单、Toast |
| **磨砂纸卡片** | 白 78%（或 86%→72% 微渐变）+ 1px 冷灰描边 + 内高光 + 冷色投影 3 级 | 所有内容卡、分组卡、映射面板 |
| **沉底内嵌** | `rgba(15,23,42,.04)` + 内阴影 1px | 分组列表底、日志预览、分段控件槽、矩阵表头槽 |

**为什么卡片不做真模糊**：一屏 5–8 张卡各带一层 `backdrop-filter`，滚动时每帧都要回读 + 重算模糊，既掉帧又让 12px 中文发虚（模糊层上叠文字本就不稳）。玻璃感来自「侧栏 + 浮层 + 柔光底」，不来自每张卡片。

### 3.4 尺度

- **圆角**：6（角标）/ 8（小按钮、导航项）/ 12（输入框、按钮）/ 16（内容卡）/ 20（对话框、浮层）/ 999（胶囊、开关）。
- **阴影**：`sh-1` `0 1px 2px rgba(15,23,42,.06)`；`sh-2` `0 6px 18px -8px rgba(15,23,42,.18), 0 1px 2px rgba(15,23,42,.05)`；`sh-3` `0 24px 60px -28px rgba(15,23,42,.35), 0 2px 6px rgba(15,23,42,.06)`；`glow` `0 6px 18px -6px rgba(0,122,255,.45)`（只给主按钮）。
- **字号**：11（角标）/ 12（hint、胶囊、表头）/ 13（正文控件、列表）/ 14（设备名、对话框标题）/ 16（区块标题）/ 20（页头）。行高：正文 1.55–1.65，标题 1.3；数字 `font-variant-numeric: tabular-nums`。
- **间距**：4 / 8 / 12 / 16 / 20（页内 24 / 32）。
- **动效**：`180ms cubic-bezier(.2,.7,.2,1)`；只做 background / box-shadow / border-color / transform / left；**不给 `backdrop-filter` 做动画**。

### 3.5 图标

线性图标、`stroke-width 1.7–1.9`、`currentColor`；默认 `--rm-text-3`，选中项 `--rm-accent`。导航不再用彩色底块。尺寸：导航 16–17，卡片内 20–24。

---

## 4. Token 定义（可直接落地）

建议新文件 `src/styles/tokens.css`（由 `App.css` 顶部 `@import`，或直接在 `App.css` 的 `:root` 内替换），页面 CSS 只引用变量。

```css
:root {
  /* 底色与表面 */
  --rm-bg: #eef2f8;
  --rm-glass: rgba(255, 255, 255, 0.62);
  --rm-card: rgba(255, 255, 255, 0.78);
  --rm-solid: #ffffff;
  --rm-sunken: rgba(19, 26, 36, 0.04);
  --rm-hover: rgba(255, 255, 255, 0.92);

  /* 描边（冷灰，不用纯黑） */
  --rm-line: rgba(19, 26, 36, 0.08);
  --rm-line-strong: rgba(19, 26, 36, 0.16);
  --rm-line-light: rgba(255, 255, 255, 0.72);

  /* 文字四级 */
  --rm-text: #131a24;
  --rm-text-2: #46536a;
  --rm-text-3: #5c6979;
  --rm-text-4: #93a0af;
  --rm-text-inv: #fff;

  /* 唯一强调色（低饱和钢蓝） */
  --rm-accent: #3a6ea5;
  --rm-accent-strong: #2a5378;
  --rm-accent-soft: rgba(58, 110, 165, 0.12);
  --rm-accent-line: rgba(58, 110, 165, 0.34);

  /* 语义三档（全部降饱和，禁用 iOS 亮色） */
  --rm-ok: #3f7f55;   --rm-ok-fill: #62a377;   --rm-ok-soft: rgba(63, 127, 85, 0.13);
  --rm-warn: #8f6c2e; --rm-warn-fill: #c8a253; --rm-warn-soft: rgba(143, 108, 46, 0.13);
  --rm-err: #a84036;  --rm-err-fill: #c96e62;  --rm-err-soft: rgba(168, 64, 54, 0.11);

  /* 圆角阶梯 */
  --rm-r-xs: 6px; --rm-r-sm: 8px; --rm-r-md: 12px;
  --rm-r-lg: 16px; --rm-r-xl: 20px; --rm-r-full: 999px;

  /* 阴影阶梯（冷色投影） */
  --rm-sh-1: 0 1px 2px rgba(19, 26, 36, 0.06);
  --rm-sh-2: 0 6px 18px -8px rgba(19, 26, 36, 0.18), 0 1px 2px rgba(19, 26, 36, 0.05);
  --rm-sh-3: 0 24px 60px -28px rgba(19, 26, 36, 0.35), 0 2px 6px rgba(19, 26, 36, 0.06);
  --rm-glow: 0 6px 16px -8px rgba(58, 110, 165, 0.38);

  /* 字号阶梯 */
  --rm-fs-11: 11px; --rm-fs-12: 12px; --rm-fs-13: 13px;
  --rm-fs-14: 14px; --rm-fs-16: 16px; --rm-fs-20: 20px;

  /* 间距阶梯 */
  --rm-sp-1: 4px; --rm-sp-2: 8px; --rm-sp-3: 12px;
  --rm-sp-4: 16px; --rm-sp-5: 20px; --rm-sp-6: 24px; --rm-sp-8: 32px;

  /* 动效 */
  --rm-ease: cubic-bezier(0.2, 0.7, 0.2, 1);
  --rm-dur: 180ms;

  /* 玻璃参数 */
  --rm-blur: blur(24px) saturate(180%);
  --rm-blur-thin: blur(20px) saturate(160%);

  /* 层叠 */
  --rm-z-menu: 20; --rm-z-dialog: 50; --rm-z-toast: 60;
}
```

**雾面底**（画在 `.app-shell` 上；这是玻璃能「有颜色可透」的前提）：

```css
.app-shell {
  background:
    radial-gradient(820px 620px at -10% 46%, rgba(58, 110, 165, 0.27), transparent 64%),
    radial-gradient(700px 480px at 4% -16%, rgba(66, 104, 150, 0.22), transparent 62%),
    radial-gradient(600px 460px at 106% -4%, rgba(101, 94, 152, 0.19), transparent 64%),
    radial-gradient(760px 560px at 74% 116%, rgba(66, 128, 140, 0.19), transparent 62%),
    linear-gradient(168deg, #f4f6f9 0%, #e9edf3 52%, #eceff4 100%);
}
```

### 4.1 迁移映射（旧值 → 新 token）

| 旧值（出处） | 新 token | 备注 |
| --- | --- | --- |
| `#007aff` / `#0062d0` / `rgba(0,122,255,.1)`（`App.css:22-24`） | `--rm-accent` / `-strong` / `-soft` | 值不变 |
| `#2f6fed`（`RemoteKeyTester.css:38,156,174,176`；`Xiaomi2ProRemote.css:338,371,376`） | `--rm-accent` | 测试组件与遥控器高亮并入 |
| `rgba(47,111,237,.7~.95)`（遥控器扇区描边 / pulseGlow） | `--rm-accent`（用 `color-mix` 或保留 rgba） | 美术层，可最后做 |
| `#248a3d` / `#34c759` / `#2da44e` / `#22c55e` | `--rm-ok` / `-fill` / `-fill` / `-fill` | **成对替换光晕色**，否则「点变了光晕没变」 |
| `#86efac` / `#f0fdf4` | `--rm-ok-soft` + `--rm-ok-fill` 描边 | 目检 |
| `#b25000` / `#d4a72c` / `#ff9f0a` | `--rm-warn` / `-fill` / `-fill` | — |
| `#d70015` / `#d1242f` / `#ff4d4f` / `#b80012` | `--rm-err` / `-fill` / 美术保留 / hover 用 `--rm-err` 加深 | — |
| `#1d1d1f` / `#1f2328` | `--rm-text` | 2 并 1 |
| `#6e6e73` / `#57606a` | `--rm-text-2` | 2 并 1 |
| `#86868b` / `#8c959f` | `--rm-text-3` | 2 并 1，且修对比度 |
| `#cbd3dd`（4 处） | `--rm-text-4` | 顺带把 `.off/.loading/.preview` 并成一条规则 |
| `#f5f5f7` / `#f7f7f9` / `#eceef3` | `--rm-bg` / `--rm-sunken` / `--rm-glass` | 侧栏由实色改玻璃 |
| `#f6f8fa`（日志底）/ `#f0f3f6` | `--rm-sunken` | 2 并 1 |
| `#edf3ff` | `--rm-accent-soft` | — |
| `rgba(0,0,0,.05 / .08)`（导航 hover/active） | `--rm-hover` + 1px 描边 | — |
| `--radius-sm` 7px（9 处） | `--rm-r-sm` 8px | **9 处同时变**，单独提交 + 侧栏截图 |
| `12px`（卡片/对话框） | `--rm-r-lg` 16px | 卡变「更圆」，与 20px 浮层成阶梯 |
| `8/10px`（私有控件） | `--rm-r-sm` 8 / `--rm-r-md` 12 | — |
| `11.5 / 12.5 / 13.5 / 14.5 / 10.5` | 就近取档 | 这批改动面大，放第二批 + 截图对比 |
| `0 0 0 3px var(--accent-soft)`（5 处） | 新增 `--rm-ring` | 统一焦点环 |
| `0 12px 40px rgba(0,0,0,.18)` | `--rm-sh-3` | — |

---

## 5. 控件规范

| 控件 | 尺寸 / 参数 | 状态 |
| --- | --- | --- |
| **主按钮** `.btn.primary` | 高 32 / 小 26；圆角 10 / 8；padding `0 12` / `0 9` | 默认：蓝色渐变 + 内高光 + `--rm-glow`；hover 加深；active 下移 0.5px；disabled 45% |
| **次级按钮** `.btn` | 同上 | 白 72% + 描边 + 内高光；hover 纯白 + `sh-2` |
| **幽灵 / 危险** `.btn.ghost` / `.danger` | 同上 | ghost 无边框；danger 红渐变；`.dialog .btn.danger` 不再单独写样式 |
| **启用态按钮** `.btn.on` | — | **新增**：accent 软底 + accent 文字 + 1px 强调描边（修 `RemoteKeyTester.tsx:432`） |
| **状态胶囊** `.pill` | 高 24；圆角 999；6px 圆点 + 12px/500 | `.ok/.warn/.err/.off` 四态；替换 `.badge`、`.device-status-pill`、`.vbcable-status` |
| **开关** `.switch` | 44×26；thumb 20 | on `#34c759`；按压时 thumb 拉长到 24；关闭 45% |
| **分段** `.seg` | 槽 3px 内边距；item 高 26 | 选中白底 + `sh-1`；替代 `.category-tabs` + `.mode-tab` |
| **输入 / 下拉** | 高 34；圆角 10 | focus：边框 `--rm-accent` + 3px `--rm-accent-soft` 环 |
| **提示条** `.callout` | 圆角 12；图标 + 标题 + 正文 | 软色底 + 1px 同色系描边（替换 `.bridge-hint` 的 3px 硬左边） |
| **对话框** | 宽 520；圆角 20；82% 白 + `blur(30px)`；遮罩 `rgba(15,23,42,.28)` + 3px 模糊 | 只有它用「最厚玻璃」 |
| **矩阵单元格** | 行高 34；表头 sticky（背景必须不透明） | 默认灰 / `.on` 正文色 / `.own` accent 软底 + 强调字 / `.sel` 2px inset 环（**不用 border**，避免行列抖动） |
| **日志预览** | 沉底内嵌 + 等宽 11px/1.75 | 级别着色：INFO 灰、WARN 琥珀、ERROR 红、DEBUG 蓝（可选） |
| **滚动条** | `::-webkit-scrollbar` 8px；thumb `rgba(15,23,42,.18)`，hover `.28`；轨道透明 | 替代 WebView2 默认宽条 |
| **焦点环** | `:focus-visible { outline: 2px solid var(--rm-accent); outline-offset: 2px }` | 全局唯一；矩阵格、`.scope-pill`、`.trigger-btn` 补齐 |

---

## 6. 页面清单

| 页面 | 改什么 |
| --- | --- |
| **连接** | 加页头（连接 / 扫描并连接遥控器…）；内容列宽由 720 → 880；设备卡改用 48px 渐变图标块；3 个状态 pill 改用 `.pill`；`.bridge-hint` → `.callout`（warn/err）；分组卡复用 `.card.group` |
| **按键映射** | 页头 + 右上动作区；作用范围改 `.sp` 胶囊（数据色 `item.color` 来自后端，**不 token 化**）；触发方式 `.seg`；动作分类 `.chip`；矩阵行高 34、表头 sticky 玻璃槽；遥控器像素图**只换 5 处强调/成功色**；整套 flex/grid 压缩规则**一行不动** |
| **通用设置** | 页头；`.form-card` → `.card.group`（行高 12px padding）；长按/双击下拉 `.select`；更新进度条沿用（宽度是动态值，别移进 CSS）；开关统一 44×26 |
| **诊断** | 双栏网格保留；自检行 `.check-row` 改 hairline + 状态 pill；`RemoteKeyTester.css` 删 fallback、`.trig-pill` 并入 `.pill`、删 `.mode-tab/.guided-*` 死样式；内联 `rgba(255,255,255,.06)` 删除 |
| **日志** | 页头 + 右侧圆形刷新（保留）；预览块改沉底 + 细滚动条；可选按级别着色 |
| **引导** | 页头；4 步列表的序号圆点用 accent-soft；`.step-title/.step-detail` 用新字号档 |
| **快捷菜单**（`public/quick-menu.html`） | 只对齐色值：`#0f172a → #0f172a`、`#64748b → #5d6b7e`、`#94a3b8` 保留、强调 `#007aff` 保留；布局与动画**不动**（含 `window.location.reload()` 热更新逻辑） |

---

## 7. 技术路线与硬约束

### 7.1 路线结论

| 路线 | 结论 |
| --- | --- |
| **A. 应用内 CSS 玻璃** | **采用**。不动 `tauri.conf.json`、不动 Rust；Win10/11 一致；回滚 = 一次 CSS revert |
| B. 系统 Mica / Acrylic | 不做。微软确认透明窗口下 CSS `backdrop-filter` 无像素可模糊；Mica 仅 Win11；透明窗口有白闪/文字发虚/掉帧历史问题 |
| C. 无边框自绘标题栏 | 不做。`titleBarStyle` 仅 macOS；Windows 丢 Snap Layouts、resize 掉帧、Win10 圆角不一致 |

### 7.2 玻璃实现的 5 条硬约束

1. **祖先链不许有 `opacity` / `filter` / `mask`**：它们会成为 *backdrop root*，让子层模糊静默失效（最容易被 React 入场淡入动画踩到）→ 玻璃容器不做 fade-in，需要过渡用 `transform` / 背景色。
2. **同屏真模糊 ≤ 3 层**：侧栏 + 1 个下拉/对话框（+ 快捷菜单窗口另算）。
3. **`blur` ≤ 20–24px、`saturate` ≤ 180%**：成本随半径和面积近似线性。
4. **玻璃层是新层叠上下文**：下拉 / 对话框 / Toast 一律 portal 到 `body` 顶层，不塞进玻璃卡。
5. **圆角与滤镜写在同一个元素**：父子各挂 radius 会出现亮边/漏光；要外壳裁切时父层 `overflow:hidden` 不带滤镜。

### 7.3 降级与可访问性

```css
@supports not (backdrop-filter: blur(1px)) {
  .glass { background: rgba(255, 255, 255, 0.94); }
}
@media (prefers-reduced-transparency: reduce) {
  .glass { backdrop-filter: none; background: rgba(255, 255, 255, 0.94); }
}
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: .001ms !important; transition-duration: .001ms !important; }
}
```

- 玻璃上的次要文字必须落在 α≥0.85 的近白面上；小字只用 `#0f172a / #475569 / #5d6b7e`。
- 低端机 / 远程桌面掉帧：设置里留「降低视觉效果」开关（写进 `config.json`），关掉滤镜与噪点。

---

## 8. 实施阶段（每阶段可独立验收、独立回滚）

| 阶段 | 内容 | 主要文件 | 验收 |
| --- | --- | --- | --- |
| **1. Token 底座** | `tokens.css` + `ui-styles.css`；`App.css` `:root` 改为引用；补 `--muted/--card` 两个 bug；旧变量保留一版 alias | `src/App.css`、新增 `src/styles/*.css` | 六页无「文字变黑 / 卡片消失」；`npm run typecheck` + `npm run build` 通过 |
| **2. 外壳** | 雾面底、玻璃侧栏、单色导航（含 `Sidebar.tsx` 删 color 数组）、统一页头、内容列宽 880、细滚动条 | `App.css`、`Sidebar.tsx`、各页 page-head | 1000×700 六页截图；切页不跳宽；侧栏玻璃在 Win10/11 各看一眼 |
| **3. 控件统一** | `.btn/.pill/.switch/.seg/.input/.callout/.chip`；删 5 套 pill、5 套次级按钮；补 `.btn.on` | `App.css`/`ui-styles.css` + 各页 replace | `grep -rE "badge|device-status-pill|vbcable-status|trigger-btn|scope-pill|shortcut-btn|mode-tab"` 结果符合预期 |
| **4. 页面精修** | 六页逐一；矩阵/遥控器高亮；日志级别着色（可选）；删死样式 | 各 `*.css` + 少量 TSX | 每页真机截图存 `docs/项目/截图/`，命名 `页面-功能.png` |
| **5. 周边一致** | `quick-menu.html` 色值；对话框（新建应用/快捷键）玻璃化；深色模式**预留** | `public/quick-menu.html`、`App.css` | 打开快捷菜单配色一致；对话框 blur 生效 |

每个 UI 阶段至少跑一次：

```powershell
npm run typecheck
npm run build
```

阶段 2/3/4 完成后在**真实运行的窗口**里看过再汇报（`npm run tauri dev` 走 Vite 热更新；`cargo run` 的话必须先 `npm run build` 再重编），截图进 `docs/项目/截图/`。

---

## 9. 风险与「不可动」清单

1. **`MappingPage.css` 的布局注释是踩坑记录，不是废话**：`.content-fill/.page-fill` 必须 `height:100%`（`App.css:802-813`）；链上 8 处 `min-height:0`（`MappingPage.css:503,514,525,537,545,552,563,586`）缺一就出整页滚动条；`.mapping-wizard` 左列 `145px`、`.merged-cols` 下限 `170px` 都是按遥控器 137px×1.3 量出来的；基础 `align-items:start` 与 `.page-fill` 下 `stretch` 是刻意不同；`.remote-art` 只保留两条 inset 阴影；sticky 表头背景必须仍是不透明色。
2. **`--remote-zoom` 是 JS↔CSS 双向契约**（`useRemoteZoom.ts:20` ↔ `MappingPage.css:595`）：动 padding / zoom 基准会让遥控器「跳一档」。
3. **遥控器像素图整体豁免 token 化**：只换 5 处语义色（`:338,371,376` 蓝、`:381,386,391` 绿），其余灰黑渐变是美术资产。
4. **后端数据色不 token 化**：`ScopeBar.tsx:40` 的 `item.color` 来自 `profiles/*.json`（`#D97757/#4D6BFE/#F59E0B`）；但 `#F59E0B` 与警告色相太近，作用域圆点容易被误读成「警告」，建议后续在档案里换一档或加白边描边。
5. **`.matrix-cell.sel` 用 inset 阴影而非 border**：防 1px 边框引起列宽/行高抖动，保持影子方案。
6. **状态点背景与光晕必须成对替换**：光晕 rgba 现在是硬编码色相（45,164,78 / 209,36,47 / 212,167,44），只换背景会出现「点变了、光晕没变」。
7. **`--radius-sm` 7→8px 同时影响 9 处**（`.btn/.select/.status-select-*/.nav-item/.field input/.shortcut-btn/.device-eat-row`），侧栏密度会变 → 单独一次提交 + 截图。
8. **遥控器引导态用了 5 处 `!important`**（`Xiaomi2ProRemote.css:365,367,381,386,391`）：改动后要在诊断页「引导模式」真机回归。
9. **CSS 加载顺序**：改造前先 `npm run build` 后确认 `dist/assets/*.css` 中 `.hint` 与 `.matrix-hint` 的先后，把结论写进规范。

---

## 10. 待拍板

| # | 问题 | 建议 |
| --- | --- | --- |
| 1 | 强调色定 `#007aff`？ | 建议是。与快捷菜单一致，改动面最小 |
| 2 | 侧栏图标改单色？ | 建议是（本稿效果）；保留六色则需新增 6 个 `--rm-nav-*`，且与「色调一致」有张力 |
| 3 | 日志页按级别上色？ | 建议做。排障时一眼看到 ERROR，约 30 行 TSX |
| 4 | 接受「不做 Mica / 无边框标题栏」？ | 建议接受，理由见 §7.1 |
| 5 | 深色模式什么时候做？ | 建议本轮只留 token 结构，下个版本再做（玻璃在深色下观感最好，但要成对设计全部语义色） |
