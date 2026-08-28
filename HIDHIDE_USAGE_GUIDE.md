# HidHide 使用调研指南（面向 RC003 返回键/音量键读取实验）

> 官方仓库：[https://github.com/nefarius/hidhide](https://github.com/nefarius/hidhide)
>
> 本文用于评估：在开启 Windows 内存完整性（HVCI）的机器上，能否用 **HidHide 隐藏遥控器 HID 设备 + 应用白名单 + 独占打开 HID 设备读取原始报告** 的方式，拿到小米蓝牙遥控器 RC003 的返回键、音量+、音量- 按键信号。
>
> 结论先行：HidHide 是 **HID 设备可见性防火墙**，只控制“哪个进程能看到设备”，**不提供读取 HID 报告的 API，也不会把 HID-over-GATT（HOGP）设备从 Windows 蓝牙 HID 驱动栈中“抢走”并交给你独占**。它可作为实验中的一个可控变量，但更可能只是一个辅助手段；能否真正读到被过滤的返回/音量键，仍依赖 Windows HID 设备栈、`hidbthle.sys` 行为以及应用侧 HID API 的实际表现，必须做最小实验验证。

---

## 目录

- [1. HidHide 是什么 / 能做什么 / 不能做什么](#1-hidhide-是什么--能做什么--不能做什么)
- [2. 与项目目标的关联分析](#2-与项目目标的关联分析)
- [3. 安装方式](#3-安装方式)
- [4. HidHideCLI 常用命令](#4-hidhidecli-常用命令)
- [5. 典型操作示例](#5-典型操作示例)
  - [5.1 查看帮助与版本](#51-查看帮助与版本)
  - [5.2 枚举设备](#52-枚举设备)
  - [5.3 隐藏某个 HID 设备实例](#53-隐藏某个-hid-设备实例)
  - [5.4 把应用加入/移出白名单](#54-把应用加入移出白名单)
  - [5.5 启用/停用隐藏（全局开关）](#55-启用停用隐藏全局开关)
  - [5.6 查询当前状态](#56-查询当前状态)
  - [5.7 批量脚本示例](#57-批量脚本示例)
- [6. 关于 HOGP / HID-over-GATT 的兼容性与注意事项](#6-关于-hogp--hid-over-gatt-的兼容性与注意事项)
- [7. 对“独占读取原始 HID 报告”目标的可行性与局限分析](#7-对独占读取原始-hid-报告目标的可行性与局限分析)
- [8. 建议实验步骤（最小验证）](#8-建议实验步骤最小验证)
- [9. 注意事项与风险](#9-注意事项与风险)
- [10. 参考资料](#10-参考资料)

---

## 1. HidHide 是什么 / 能做什么 / 不能做什么

### 1.1 是什么

HidHide（[https://github.com/nefarius/hidhide](https://github.com/nefarius/hidhide)）是 Nefarius 开发的一款 **Windows 输入设备防火墙（Gaming Input Peripherals Device Firewall）**，是 HidGuardian 的继任者，从零重新实现。

- 它是一个 **内核态过滤驱动（filter driver）**，适用于 Windows 10/11 64 位。
- 它主要面向 **游戏手柄、摇杆、方向盘、游戏控制器** 等输入设备，用于解决“物理设备 + 虚拟设备同时被应用看到”导致的双输入问题。
- 安装包同时包含：内核驱动、图形配置客户端（HidHideClient）、命令行客户端（HidHideCLI）、驱动安装工具等。

### 1.2 能做什么

官方 README 与文档描述的核心能力：

- **按应用隐藏设备**：可以拒绝某个应用访问一个或多个 HID/XInput 设备，使设备对该应用不可见。
- **应用白名单（cloak / whitelist）**：指定哪些应用即使设备被隐藏，也仍然可以看见并访问它。
- **设备黑名单（blacklist）**：按设备实例路径（device instance path）指定要隐藏的具体设备。
- **全局开关**：启用/停用整个隐藏机制。
- **反向白名单（inverse whitelist）**：CLI 提供 `inv-on` / `inv-off`，可切换白名单语义（较少用到）。
- **会话级黑名单（session blacklist，源码 master 已合入，正式 Release 是否包含需确认）**：应用可通过 IOCTL 在运行时把自己独占的设备加入“进程级黑名单”，进程退出后自动释放，适合 remapper 类应用。

### 1.3 不能做什么（重要）

- **不读取 HID 报告**：HidHide 没有提供“读取原始输入报告”的 API。它只控制设备枚举/打开是否可见，报告数据仍要通过 Windows HID API（如 `HidD_GetInputReport`、`ReadFile`、`Windows.Devices.HumanInterfaceDevice.HidDevice`）或 Raw Input 获取。
- **不拦截/不重写系统对键盘鼠标类设备的输入处理**：官方 FAQ 明确说明，**无法隐藏鼠标、键盘、触摸板/触摸屏**；“键盘和鼠标输入走不同的路径，HidHide 的阻止机制无法像手柄那样干预”。官方 issue 也确认该限制长期有效。
- **不保证隐藏后设备会从系统驱动栈中消失**：它是过滤驱动，影响的是“应用能否打开/看到设备”，不是把设备从 Windows 输入管线中物理移除。
- **不替代 BLE 协议层**：对于 HID-over-GATT（HOGP）设备，Windows 由 `hidbthle.sys` 等驱动负责蓝牙 HID 接入；HidHide 是否能在该栈上按预期隐藏、以及隐藏后应用能否读到过滤前的报告，官方资料没有明确承诺。

### 1.4 官方文档中的“设备防火墙”定位

> With HidHide it is possible to deny a specific application access to one or more human interface devices, effectively hiding a device from the application.
>
> 简单说：它让“某个应用看不到某个设备”，而不是“帮你读取设备”。

---

## 2. 与项目目标的关联分析

| 项目目标 | HidHide 能提供的帮助 | 局限 / 不确定性 |
| --- | --- | --- |
| 让系统过滤掉返回/音量键 | 可能把 RC003 的 HID 节点对系统普通应用隐藏，避免系统/其他应用抢占或过滤 | 官方不支持键盘/鼠标类；遥控器属于“Consumer Control / HID 键盘复合节点”，能否被 HidHide 拦截需实测 |
| 让我们的应用仍然能打开设备 | 通过 `app-reg` 把应用加入白名单，使白名单进程可以访问被隐藏设备 | 白名单基于“可执行文件完整路径 + 进程镜像”，路径变化需重新注册 |
| 独占打开 HID 设备读取原始报告 | HidHide 不提供读取 API，但隐藏后可能减少系统驱动/其他应用对设备的竞争 | 关键问题是：即使白名单应用能打开，**读到的报告是 Windows HID 栈过滤后的报告还是原始报告**，官方无保证 |
| 在 HVCI 下绕过 Frida 注入限制 | HidHide 是已签名的内核过滤驱动 + 用户态 CLI/API，**不需要注入目标进程**，因此原则上不受 HVCI 对 Frida 注入的拦截影响 | 安装内核驱动本身受 Windows 驱动签名策略约束；HVCI 对第三方驱动兼容性有额外要求（需要 HVCI 兼容签名/代码），需实测安装是否成功 |
| 与现有 core-hid / Raw Input 链路共存 | 可以只隐藏特定设备实例，白名单只放自己的读取进程，理论上不影响其它 HID 设备 | 隐藏后遥控器整体对系统不可见，可能影响普通按键（方向/OK/Home 等）的 Raw Input 路径，需要分场景测试 |

> 重点提醒：HidHide 的核心价值是 **“让设备对非白名单应用不可见”**，它不能把 `hidbthle.sys` 已经过滤掉的报告“变回来”。RC003 的返回/音量键问题本质上是 Windows HID-over-GATT 驱动栈的报告分发/过滤问题；HidHide 是否能绕开该问题，必须通过“隐藏 + 白名单 + HID API 读取”的最小实验验证，不能仅凭文档推断。

---

## 3. 安装方式

### 3.1 官方推荐安装（x64）

1. 下载最新 Release：
   - GitHub Releases：https://github.com/nefarius/HidHide/releases/latest
   - 当前官方 Release 是 `HidHide_1.5.230_x64.exe`（v1.5.230.0，2024-05-11 发布；源码 master 更新至 2026 年，包含 session blacklist 等新特性，但可能未打进正式安装包）。
2. 按需安装 Visual C++ Redistributable（官方文档要求）。
3. 以管理员身份运行安装程序。
4. 安装完成后 **重启系统**（驱动安装/卸载后可能要求重启）。
5. 安装后可在开始菜单找到 **HidHide Configuration Client**。

> 注意：HidHide 和旧版 HidGuardian **不能同时安装**，如有旧版需先卸载并重启。

### 3.2 Chocolatey（可选）

HidHide 在 Chocolatey 社区仓库有 `hidhide` 包。安装示例：

```powershell
choco install hidhide -y
```

### 3.3 ARM64 手动安装（如需要）

官方文档提供 ARM64 手动安装方式：下载 `HidHide_ARM64.zip`，用 `nefcon` 安装驱动，再下载 ARM64 客户端/CLI。普通 x64 机器不需要。

### 3.4 安装后的典型路径

根据仓库 `INSTALL_LAYOUT.md` 的“canonical”布局，现代安装路径为：

```text
%ProgramFiles%\Nefarius Software Solutions\HidHide\
├── HidHide.sys / HidHide.inf / HidHide.cat
├── HidHideClient.exe
├── HidHideCLI.exe
└── nefconw.exe
```

> 早期版本/第三方资料中也可能出现 `C:\Program Files\Nefarius Software Solutions e.U.\HidHide`、`...\HidHide\x64\HidHideCLI.exe` 等路径。实际以本机安装后的路径为准，可用 `Get-Command HidHideCLI` 或直接检查上述目录确认。

---

## 4. HidHideCLI 常用命令

以下命令清单来自仓库源码（`HidHideCLI/src/Commands.cpp`、`HidHideCLI/HidHideCLI.rc`）以及社区整理的 CLI 用法。CLI 命令都使用 `--` 前缀，可多条命令在同一行/同一批中顺序执行，最后统一保存配置。

> 注意：HidHideCLI 的“开关”类命令实际是 **`--cloak-on` / `--cloak-off` / `--cloak-state`**，不是 `set-active`。GitHub README / 官方 docs 没有单独列出 `set-active`；你在其它第三方教程或旧版资料中可能看到 `set-active` 的说法，但以当前源码和 `--help` 输出为准。

| 命令 | 参数 | 作用 |
| --- | --- | --- |
| `--help` | 无 | 显示所有支持的命令 |
| `--version` | 无 | 显示 CLI 版本 |
| `--cloak-on` | 无 | 启用设备隐藏（全局隐藏开关） |
| `--cloak-off` | 无 | 停用设备隐藏（恢复设备对全部应用可见） |
| `--cloak-state` | 无 | 查询当前隐藏开关状态（输出 `--cloak-on` 或 `--cloak-off`） |
| `--cloak-toggle` | 无 | 切换隐藏开关状态 |
| `--app-list` | 无 | 列出已注册的白名单应用 |
| `--app-reg` | `"<应用完整路径>"` | 把应用加入白名单，允许其访问被隐藏设备 |
| `--app-unreg` | `"<应用完整路径>"` | 把应用移出白名单 |
| `--app-clean` | 无 | 清理已不存在的白名单条目 |
| `--dev-all` | 无 | 列出当前所有 HID 设备（JSON 格式，包含 device instance path 等信息） |
| `--dev-gaming` | 无 | 列出被识别为“游戏设备”的 HID 设备（JSON 格式） |
| `--dev-hide` | `"<device instance path>"` | 隐藏指定设备实例 |
| `--dev-unhide` | `"<device instance path>"` | 取消隐藏指定设备实例 |
| `--dev-list` | 无 | 列出当前已隐藏的设备实例路径 |
| `--inv-on` | 无 | 开启反向白名单（inverse whitelist） |
| `--inv-off` | 无 | 关闭反向白名单 |
| `--inv-state` | 无 | 查询反向白名单状态 |
| `--cancel` | 无 | 不保存配置更改并退出（主要用于交互模式） |

### 4.1 关于 `set-active`

用户任务中提到 `set-active`。调研结果：

- 当前 HidHideCLI 源码中 **没有 `set-active` 命令**，对应功能由 `--cloak-on` / `--cloak-off` / `--cloak-toggle` / `--cloak-state` 提供。
- 有些第三方文章或旧版使用示例可能写成 `set-active true` / `set-active false`，但那不是官方当前 CLI 的语法。
- 建议在你的实验脚本中统一使用 `--cloak-on` / `--cloak-off`，并在实际机器上先运行 `HidHideCLI.exe --help` 确认命令列表。

### 4.2 命令行为说明

- CLI 支持一次传入多条命令，例如：
  ```text
  HidHideCLI.exe --app-reg "C:\path\to\app.exe" --dev-hide "HID\VID_...&...\..." --cloak-on
  ```
  多条命令会顺序执行，退出时统一应用配置。
- 修改配置时，**如果图形客户端 HidHideClient 正在运行，CLI 可能遇到 Access Denied / 配置冲突**。社区维护者建议：使用 CLI 前先关闭 HidHideClient。
- CLI 通常 **不需要管理员权限**（社区维护者明确说过 “No Admin required”），但安装驱动/首次配置可能需要管理员；具体以实际机器为准。
- 白名单要求完整可执行文件路径，且必须是 `.exe` / `.com` / `.bin` 等可执行文件；路径不能是相对路径。
- 隐藏/取消隐藏按 **设备实例路径（device instance path）** 精确匹配，不是按设备型号/名称匹配。

---

## 5. 典型操作示例

以下命令均假设 HidHideCLI 位于 PATH 中，或使用完整路径，例如：

```powershell
# 假设安装在默认路径
$cli = "$env:ProgramFiles\Nefarius Software Solutions\HidHide\HidHideCLI.exe"
```

### 5.1 查看帮助与版本

```powershell
& $cli --help
& $cli --version
```

### 5.2 枚举设备

```powershell
# 列出所有 HID 设备
& $cli --dev-all

# 只列出被识别为游戏设备的 HID 设备
& $cli --dev-gaming
```

`--dev-all` / `--dev-gaming` 输出为 JSON 数组，字段包括：

- `present`：设备当前是否在线
- `gamingDevice`：是否被识别为游戏设备
- `symbolicLink`：HID 符号链接，如 `\\?\HID#...`
- `vendor` / `product` / `serialNumber`
- `usage`：HID usage 信息
- `description`
- `deviceInstancePath`：设备实例路径，`dev-hide` 使用这个值
- `baseContainerDeviceInstancePath` / `baseContainerDeviceCount` 等

示例输出结构：

```json
[
  {
    "present" : true,
    "gamingDevice" : false,
    "symbolicLink" : "\\\\?\\HID#VID_2717&PID_32B8#...",
    "vendor" : "Xiaomi",
    "product" : "Xiaomi Bluetooth Remote Control",
    "serialNumber" : "",
    "usage" : "...",
    "description" : "...",
    "deviceInstancePath" : "HID\\VID_2717&PID_32B8\\...",
    "xusbDeviceInstancePath" : "",
    "baseContainerDeviceInstancePath" : "...",
    "baseContainerClassGuid" : "...",
    "baseContainerDeviceCount" : 1
  }
]
```

> 提示：RC003 走蓝牙 HOGP 时，设备实例路径很可能形如 `HID\{00001124-0000-1000-8000-00805f9b34fb}_VID&0002xxxx_PID&xxxx\...`，即包含蓝牙 HOGP 的经典 GUID `00001124-...`。请以实际 `--dev-all` 输出为准。

### 5.3 隐藏某个 HID 设备实例

```powershell
# 用 --dev-all / --dev-gaming 查到的 deviceInstancePath
& $cli --dev-hide "HID\VID_2717&PID_32B8\7&xxxxxxxx&0&0000"
```

隐藏后查询隐藏列表：

```powershell
& $cli --dev-list
```

取消隐藏：

```powershell
& $cli --dev-unhide "HID\VID_2717&PID_32B8\7&xxxxxxxx&0&0000"
```

### 5.4 把应用加入/移出白名单

```powershell
# 注册白名单（必须是完整路径）
& $cli --app-reg "C:\Program Files\RemoteMic\remote-mic.exe"

# 查看白名单
& $cli --app-list

# 移出白名单
& $cli --app-unreg "C:\Program Files\RemoteMic\remote-mic.exe"
```

> 注意：如果应用被移动/重命名，需要重新 `app-reg`。白名单按应用可执行文件完整路径匹配，而不是只按进程名匹配。

### 5.5 启用/停用隐藏（全局开关）

```powershell
# 启用隐藏
& $cli --cloak-on

# 停用隐藏
& $cli --cloak-off
```

### 5.6 查询当前状态

```powershell
# 查询隐藏开关
& $cli --cloak-state

# 查询已隐藏设备
& $cli --dev-list

# 查询白名单
& $cli --app-list

# 查询反向白名单状态
& $cli --inv-state
```

### 5.7 批量脚本示例

PowerShell 脚本示例：把实验程序加入白名单、隐藏 RC003 设备、启用隐藏。

```powershell
$cli = "$env:ProgramFiles\Nefarius Software Solutions\HidHide\HidHideCLI.exe"
$remoteMicExe = "C:\Program Files\RemoteMic\remote-mic.exe"
$rc003InstancePath = "HID\VID_2717&PID_32B8\7&xxxxxxxx&0&0000"

# 先关闭图形客户端，避免配置冲突
Get-Process HidHideClient -ErrorAction SilentlyContinue | Stop-Process

& $cli --app-reg "$remoteMicExe" --dev-hide "$rc003InstancePath" --cloak-on
```

命令行一次性示例：

```bat
"C:\Program Files\Nefarius Software Solutions\HidHide\HidHideCLI.exe" --app-reg "C:\Program Files\RemoteMic\remote-mic.exe" --dev-hide "HID\VID_2717&PID_32B8\7&xxxxxxxx&0&0000" --cloak-on
```

---

## 6. 关于 HOGP / HID-over-GATT 的兼容性与注意事项

### 6.1 官方资料现状

- 官方 README / docs / FAQ 主要围绕 **USB 游戏手柄、方向盘、joystick** 等设备，没有专门的 HOGP（HID-over-GATT）兼容性文档。
- 官方 FAQ 明确说明：**不能隐藏鼠标、键盘、触摸板/触摸屏**。对蓝牙 HOGP 键盘/遥控器这类设备，官方没有承诺支持。
- 仓库/Issue 中没有发现针对 HOGP 的官方支持声明，但社区讨论中出现过蓝牙手柄（DualSense）的实例路径形如：
  ```
  HID\{00001124-0000-1000-8000-00805f9b34fb}_VID&0002054c_PID&09cc\...
  ```
  这说明 HidHide 的设备枚举确实能“看到”某些蓝牙 HOGP 设备节点，并可能对其隐藏。但“能枚举到”不等于“隐藏后能读到原始报告”。

### 6.2 对 RC003 的预期

- RC003 是小米蓝牙遥控器，返回键/音量键走 HID-over-GATT（HOGP）。
- Windows 下这类设备通常由 `hidbthle.sys` 加载为 HID 设备，并可能有多个 HID 集合（键盘、Consumer Control 等）。
- 如果 HidHide 的设备列表能看到该 HID 节点，理论上可以对它执行 `dev-hide` 并 `app-reg` 白名单应用；但需要验证：
  1. 该节点是否真的受 HidHide 过滤驱动影响；
  2. 隐藏后设备是否从系统输入管线消失（可能影响普通按键）；
  3. 白名单应用用 HID API 打开后，能否收到返回/音量键的原始报告。

### 6.3 已知注意事项

- **重新配对会改变设备实例路径**：社区/官方确认，蓝牙设备取消配对再重新配对后，Windows 生成的 Instance ID 可能变化，之前 `dev-hide` 的条目会失效，需要重新隐藏。对 RC003 实验而言，尽量保持配对稳定。
- **“Gaming devices only” 只是 UI 过滤**：在图形客户端取消勾选“Gaming devices only”只影响设备列表显示，不改变驱动功能；如果设备没出现在列表，应关闭该过滤查看。
- **不要同时安装 HidGuardian**。
- **杀毒软件可能干扰白名单**：官方文档提到 Kaspersky 会破坏进程识别逻辑；其它安全软件也可能影响 `app-reg` 效果。
- **HVCI / 驱动签名**：HidHide 是内核驱动。虽然其用户态 CLI/API 不涉及 Frida 式注入，但内核驱动在 HVCI 开启的系统上必须满足 Windows 的 HVCI 兼容签名要求；请先在目标机器实测安装/重启是否成功。

---

## 7. 对“独占读取原始 HID 报告”目标的可行性与局限分析

### 7.1 HidHide 的角色边界

- HidHide 解决的是 **“设备对哪些应用可见”**，不是 **“如何读取报告”**。
- 如果 Windows HID 栈（`hidbthle.sys`）已经对返回/音量键做了过滤或转换，HidHide 隐藏设备并不能保证把过滤后的路径取消。
- HidHide 可能带来的帮助：
  - 让系统普通应用/输入管线不再与你的应用竞争同一个设备；
  - 如果你的应用是白名单应用，设备隐藏后它仍可尝试打开 HID 设备；
  - 如果设备被系统当作“游戏设备”或普通 HID 节点管理，`dev-hide` + `app-reg` 可以提供一个干净的访问通道。
- HidHide 不能提供的帮助：
  - 不提供“独占读报告”API；
  - 不提供“把 HOGP 报告从 `hidbthle.sys` 之前截获”的能力（至少官方没有这种承诺）；
  - 不能像 Frida 那样注入 `WUDFHost` / `hidbthle` 进程去 hook 内部 IOCTL。

### 7.2 读取 HID 报告的技术路径

即使 HidHide 隐藏成功，仍需要应用自己使用 Windows HID 接口读取：

- **Win32 HID API（`hidsdi.h`）**：
  - `HidD_GetHidGuid` + `SetupDiGetClassDevs` 枚举；
  - `CreateFile` 打开设备符号链接；
  - `HidD_GetInputReport` 主动读取输入报告；
  - `ReadFile` 异步读取输入报告。
- **WinRT API**：`Windows.Devices.HumanInterfaceDevice.HidDevice`，UWP / WinRT 桥接方式。
- **Rust 生态**：`hidapi`、`windows` crate、`core-hid` 现有 Raw Input 逻辑等。

> 关键判断标准：在隐藏 + 白名单后，用 `CreateFile` 打开 RC003 的 HID 节点能否成功；按返回/音量键时 `ReadFile` 是否能收到包含 `0x00F1` / `0x0080` / `0x0081` 等 usage 的原始报告。

### 7.3 可行性分级

| 方案 | 可行性 | 说明 |
| --- | --- | --- |
| HidHide 隐藏设备 + 白名单应用 + HID API 读取 | **待验证（低-中概率）** | 官方不支持键盘/鼠标类，HOGP 无明确保证；必须实测 |
| 仅用 HidHide 让应用独占设备 | 可行（针对受支持设备） | 对游戏手柄类成熟；对 RC003 需验证 |
| 用 HidHide 替代 Frida 注入读取被过滤报告 | 不成立 | HidHide 不读取/不劫持报告，不能替代 Frida Tap |
| 用 BLE GATT 直连读取原始报告 | 更可靠的方向 | 如果 HOGP 报告确实被 Windows 过滤，直接走 BLE GATT 读取（不依赖 HID 驱动）可能是更稳的替代方案 |

### 7.4 HVCI 相关

- HVCI 拦截的是“注入受保护进程/加载未签名代码”类行为，HidHide 的常规使用（用户态 CLI 配置 + 驱动过滤）不涉及注入，理论上不受 HVCI 拦截。
- 但内核驱动安装本身是更高权限操作，且 HVCI 对驱动兼容性有要求。目标机器能否正常安装、加载 HidHide 驱动，需要先在真实环境验证。

---

## 8. 建议实验步骤（最小验证）

1. **基线**：
   - 确认 HVCI 状态；
   - 用 `Get-PnpDevice` / 设备管理器查看 RC003 的 HID 节点；
   - 用现有 Raw Input 日志确认返回/音量键当前收不到（基线）。
2. **安装 HidHide**：
   - 从 [https://github.com/nefarius/hidhide](https://github.com/nefarius/hidhide) 下载最新 Release；
   - 安装后重启；
   - 运行 `HidHideCLI.exe --help` 确认命令列表。
3. **枚举设备**：
   - 运行 `--dev-all`（必要时 `--dev-gaming`），找到 RC003 的 `deviceInstancePath`。
4. **配置**：
   - `--app-reg "<你的实验程序.exe>"`；
   - `--dev-hide "<RC003 deviceInstancePath>"`；
   - `--cloak-on`；
   - 关闭图形客户端以避免冲突。
5. **读取测试**：
   - 用最小 C#/Rust 程序通过 HID API 打开设备；
   - 按返回/音量+ / 音量-，打印原始报告；
   - 同时观察系统是否仍能收到普通按键（方向/OK 等），判断影响范围。
6. **回退/清理**：
   - `--dev-unhide` + `--cloak-off` 恢复；
   - 若失败，考虑 BLE GATT 直连方案。

> 详细的实验计划可参考同目录 `HIDHIDE_EXPERIMENT_PLAN.md`。

---

## 9. 注意事项与风险

- **不要修改用户的全局 HidHide 配置**：官方 README 提醒第三方部署应保守，只新增必要条目，不要假设独占配置，否则恢复需要人工干预。
- **白名单按完整路径匹配**：应用升级/移动/重命名后需要重新 `app-reg`。
- **CLI 与 GUI 不要同时操作**：可能冲突或 Access Denied。
- **设备实例路径不稳定**：蓝牙重新配对可能导致路径变化，隐藏失效。
- **键盘/鼠标/触摸板不受支持**：如果 RC003 被系统归类为键盘或 Consumer Control，HidHide 可能无效。
- **隐藏后系统整体看不到遥控器**：可能影响语音键、方向键等现有功能，实验时注意回退。
- **杀毒软件/安全软件干扰**：可能影响白名单生效。
- **HidHide 官方维护状态**：README 提到当前没有能力做重大开发，遇到问题优先查 GitHub Issues / Discord。

---

## 10. 参考资料

- HidHide GitHub 仓库（官方）：[https://github.com/nefarius/hidhide](https://github.com/nefarius/hidhide)
- HidHide README：https://github.com/nefarius/HidHide/blob/master/README.md
- HidHide 官方文档 About：https://docs.nefarius.at/projects/HidHide/
- HidHide Simple Setup Guide：https://docs.nefarius.at/projects/HidHide/Simple-Setup-Guide/
- HidHide FAQ：https://docs.nefarius.at/projects/HidHide/FAQ/
- HidHide Developer Guide（IOCTL / session blacklist）：https://raw.githubusercontent.com/nefarius/HidHide/master/DEVELOPER.md
- HidHide CLI 讨论（“There is no documentation for the command line client”）：https://github.com/nefarius/HidHide/discussions/60
- HidHide 蓝牙重配对导致实例路径变化：https://github.com/nefarius/HidHide/discussions/63
- HidHide “不用于键盘/鼠标” 官方说明：https://github.com/nefarius/HidHide/issues/15
- 第三方 CLI 用法整理（tales-from-darkenedroom）：https://www.tales-from-darkenedroom.com/post/controllers-and-retro-sims-part-10-vjoy-joystick-gremlin-hidhide-command-line-options
- Windows HID API（hidsdi.h）：https://learn.microsoft.com/en-us/windows/win32/api/hidsdi/
- Windows.Devices.HumanInterfaceDevice.HidDevice：https://learn.microsoft.com/en-us/uwp/api/windows.devices.humaninterfacedevice.hiddevice
- 同项目实验计划：`HIDHIDE_EXPERIMENT_PLAN.md`
