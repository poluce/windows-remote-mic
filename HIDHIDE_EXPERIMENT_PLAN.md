# HidHide + 独占读取实验计划

> 目标：验证是否可以通过 **HidHide 隐藏遥控器 + 应用白名单 + HID API 独占读取**，在开启 HVCI（内存完整性）的机器上获取小米遥控器 RC003 的返回、音量+、音量- 原始 HID 报告。
> 状态：计划中 / 未开始
> 关联问题记录：[docs/项目/问题记录-读取返回音量键.md](docs/项目/问题记录-读取返回音量键.md)

---

## 1. 背景

- RC003 的返回 / 音量键走 HID-over-GATT（HOGP）。
- Windows `hidbthle.sys` 会先处理/过滤这些报告，普通应用无法通过 Raw Input 收到。
- 本机开启 HVCI，Frida 注入 WUDFHost 被拦截。
- 本机实测 Raw Input 收不到返回/音量键。
- 待验证：HidHide 能否把 HOGP 设备从系统驱动手中“抢走”，让应用独占读取原始报告。

---

## 2. 实验假设

1. HidHide 可以隐藏 RC003 的 HID 设备节点，使系统驱动（`hidbthle.sys`）不再处理/过滤报告。
2. 隐藏后，白名单应用可以通过 HID API 打开设备。
3. 应用打开的句柄能读到包含返回（0x00F1）、音量+（0x0080）、音量-（0x0081）的原始 HID 报告。

---

## 3. 前置条件

- Windows 10/11 64 位。
- 小米遥控器 RC003 已通过蓝牙配对。
- 管理员权限（安装驱动/运行实验）。
- 可选：关闭 Frida/HID Tap 相关逻辑，避免干扰。

---

## 4. 实验步骤

### 阶段 0：基线确认

- [ ] 确认系统 HVCI 状态（`Win32_DeviceGuard`）。
- [ ] 确认遥控器 HID 设备节点（设备管理器 / `Get-PnpDevice`）。
- [ ] 用 Raw Input 日志确认当前收不到返回/音量（基线）。

### 阶段 1：安装 HidHide

- [ ] 下载 HidHide（https://github.com/nefarius/HidHide）。
- [ ] 安装 HidHide 驱动 + 客户端/CLI。
- [ ] 确认驱动服务运行。

### 阶段 2：识别遥控器设备

- [ ] 通过 HidHideClient / HidHideCLI 枚举设备。
- [ ] 找到 RC003 对应的 HID 设备实例路径（含 `HID#00001812...` / `VID_012717&PID_32b8`）。

### 阶段 3：隐藏设备 + 白名单

- [ ] 用 HidHideCLI 将遥控器设备加入隐藏列表（`dev-hide`）。
- [ ] 将实验程序/应用加入白名单（`app-reg`）。
- [ ] 开启全局隐藏开关（`set-active true`）。

### 阶段 4：编写最小读取程序

- [ ] 用 Rust 或 C# 编写 HID 读取程序：
  - 通过 `Windows.Devices.HumanInterfaceDevice.HidDevice` 打开设备。
  - 或使用 `HidD_GetInputReport` / `CreateFile` + `ReadFile`。
- [ ] 读取输入报告，打印原始字节。
- [ ] 按遥控器返回/音量键，观察报告内容。

### 阶段 5：结果判定

- [ ] 能否打开设备（成功/ AccessDenied / 找不到设备）。
- [ ] 能否读到返回/音量 usage（0x00F1 / 0x0080 / 0x0081）。
- [ ] 是否受 HVCI 影响（应不受影响，因为是用户态 HID API）。

### 阶段 6：集成到项目（如果成功）

- [ ] 在 `core-hid` 增加 HID 独占读取模块。
- [ ] 启动时检测 HidHide 是否可用。
- [ ] 隐藏设备 + 白名单当前进程。
- [ ] 读取报告并转成 `RawInputEvent` / 按键事件。
- [ ] 与现有按键映射链路对接。
- [ ] 处理失败回退（HidHide 不可用时保持现状）。

---

## 5. 风险与对策

| 风险 | 影响 | 对策 |
| --- | --- | --- |
| HidHide 不兼容 HOGP 设备 | 无法隐藏/打开 | 改用 BLE GATT 直连方案 |
| 隐藏后设备从系统消失，影响语音/普通键 | 遥控器整体不可用 | 先做最小实验，避免影响正式功能 |
| HID API 打开后仍读不到过滤前报告 | 方案无效 | 记录实际报告，尝试 `HidD_GetInputReport` / `ReadFile` 两种方式 |
| 白名单配置复杂/持久化问题 | 每次启动需重新配置 | 使用 HidHideCLI 脚本化配置 |
| 与 Frida HID Tap 冲突 | 双通道重复/干扰 | 实验期间禁用 Frida Tap |

---

## 6. 验收标准

- [ ] 在 HVCI 开启的机器上，应用能通过 HID API 打开 RC003。
- [ ] 按返回键能读到 `0x00F1` 报告。
- [ ] 按音量+/音量- 能读到 `0x0080` / `0x0081` 报告。
- [ ] 普通键（方向/OK/Home 等）仍能通过 Raw Input 获取，无双触发。

---

## 7. 时间估算

| 阶段 | 预计耗时 |
| --- | --- |
| 基线确认 | 0.5 天 |
| 安装 HidHide + 识别设备 | 0.5 天 |
| 隐藏 + 白名单 | 0.5 天 |
| 最小读取程序 | 1 天 |
| 结果判定 / 调整 | 0.5 天 |
| 集成（如成功） | 2-3 天 |

---

## 8. 参考资料

- HidHide：https://github.com/nefarius/HidHide
- HidHideCLI 文档：https://docs.nefarius.at/projects/HidHide/
- Windows HID API：https://learn.microsoft.com/en-us/windows/win32/api/hidsdi/
- 问题记录：docs/项目/问题记录-读取返回音量键.md
