# TODO — Windows 无线麦（Remote Mic）

> 关联文档：[PLANNING.md](./PLANNING.md)
> 技术栈：Rust 核心 + Tauri 2 / TypeScript / React
> 目标设备：小米蓝牙遥控器 2 Pro / RC003，后续可扩展 RC001

图例：
- `[ ]` 待办
- `[x]` 已完成

---

## 0. 前期调研与基线复验

- [ ] 梳理 RC003/RC001 的公开蓝牙协议资料（ATVV、HID、ADPCM）
- [ ] 整理 Windows BLE/HID/虚拟声卡实现路线的可行性资料
- [ ] 整理 ATVV 协议关键事实（UUID、ADPCM、会话命令、HID usage）
- [ ] 确认 GPL-3.0 / 第三方组件许可影响
- [ ] 准备真实 RC003 遥控器与 Windows 10/11 x64 测试机
- [ ] M0：用真机 RC003 完成“配对 → 按键 → 语音出字”链路行为采集
- [ ] 记录真机 HID 原始数据与语音链路日志，作为 Rust 实现对照

## 1. 工程初始化

- [x] 初始化 Git 仓库并推送到 `windows-remote-mic`
- [x] 创建 Rust workspace（已含 core-* crates 骨架）
  - [ ] `crates/core-ble`
  - [ ] `crates/core-atvv`
  - [ ] `crates/core-audio`
  - [ ] `crates/core-hid`
  - [ ] `crates/core-input`
  - [ ] `crates/core-mapping`
  - [ ] `crates/core-config`
  - [ ] `crates/core-diagnostics`
  - [ ] `crates/app`
- [x] 初始化 Tauri 2 + React + TypeScript 前端（前后端 ping 通）
- [ ] 配置 `cargo fmt` / `clippy` / `rustfmt` 基础规范
- [ ] 配置 GitHub Actions：Rust build + test + Tauri bundle
- [ ] 建立日志目录与统一错误上报结构
- [ ] 建立配置目录 `%LOCALAPPDATA%\RemoteMic\RC003`

## 2. 核心模块：BLE / ATVV

- [ ] `core-ble`
  - [ ] WinRT BLE 扫描并精确匹配 RC003 设备名
  - [ ] GATT 枚举使用 `BluetoothCacheMode.UNCACHED`
  - [ ] ATVV Service/Characteristic UUID 常量定义
  - [ ] 连接、断开、自动重连、超时处理
  - [ ] 单实例 / 并发连接保护
- [ ] `core-atvv`
  - [ ] ATVV 能力协商
  - [ ] MIC_OPEN / STREAM_START / STREAM_STOP 会话控制
  - [ ] MIC_EXTEND / 长时间语音租期处理
  - [ ] 16kHz IMA/DVI ADPCM 解码
  - [ ] 同步包 / predictor / step index 重置
  - [ ] 停止后 0.3 秒尾部数据丢弃策略

## 3. 核心模块：音频

- [ ] `core-audio`
  - [x] WASAPI 音频端点枚举（windows-rs / IMMDeviceEnumerator，待 Windows 真机验证）
  - [ ] 用户选择输出端点并持久化
  - [x] 16kHz → 48kHz 重采样
  - [x] +10dB 增益 / 20Hz DC 阻挡
  - [x] 立体声端点声道复制（含限幅）
  - [ ] 测试音
  - [ ] 输出设备失配/断开时失败关闭
- [ ] 虚拟声卡方案
  - [ ] 调研 VB-CABLE 安装引导流程
  - [ ] 可选：随包提供官方 VB-CABLE + SHA-256 校验 + UAC 安装
  - [ ] 可选：评估自研虚拟音频驱动可行性

## 4. 核心模块：HID / 按键

- [ ] `core-hid`
  - [ ] Raw Input 捕获 RC003 键盘事件
  - [ ] HID 路径校验，避免同型号多设备串扰
  - [ ] RC003 物理 usage 映射表（方向/OK/Home/Menu/TV/Power/返回/音量/语音键）
  - [ ] Windows Raw Input 缺失键（返回 `0xF1`、音量 `0x80/0x81`）旁路方案
  - [ ] 可选：Frida/WUDFHost 旁路可行性验证
  - [ ] 记录按键采集与回放工具
- [ ] `core-input`
  - [ ] `SendInput` 键盘事件注入
  - [ ] 防止一次按键双触发
  - [ ] 组合快捷键（Win 键、Ctrl、Alt 等）
  - [ ] 打开应用动作
  - [ ] 系统音量 / 播放控制
- [ ] `core-mapping`
  - [ ] 13 键默认映射表
  - [ ] 单击 / 双击 / 长按手势
  - [ ] 按住重复策略
  - [ ] 配置热加载（运行中修改映射立即生效）
  - [ ] 自定义快捷键录制 UI

## 5. 核心模块：系统语音输入优先 / 第三方 IME 兼容

- [ ] `core-ime`
  - [ ] Windows 自带语音输入（Win+H）作为基础链路，优先级最高
  - [ ] 触发 `Win+H` 系统语音键入
  - [ ] 引导用户把语音输入麦克风设为 `CABLE Output`
  - [ ] 诊断：Windows 语音语言包 / 在线语音可用性 / 麦克风权限
  - [ ] 抽象 IME 接口（为后续第三方输入法预留）
  - [ ] 豆包输入法：`ralt` 按住 / `ralt+space` 切换（可选扩展）
  - [ ] 微信输入法兼容（可选扩展）
  - [ ] 注入事件被 IME 识别为真实按键的兼容层（第三方 IME 需要时再做）
  - [ ] 语音键按下/释放与音频流生命周期同步

## 6. 配置 / 日志 / 诊断

- [ ] `core-config`
  - [ ] 原子化 JSON 写入（临时文件 + fsync + rename）
  - [ ] 配置迁移 / 旧版兼容
  - [ ] 配置损坏时保留最后一份有效配置
- [ ] `core-diagnostics`
  - [ ] BLE 状态诊断
  - [ ] 音频端点检测
  - [ ] 按键采集向导
  - [ ] Raw Input 广域探针
  - [ ] 日志查看与导出
  - [ ] 隐私边界：不记录蓝牙地址 / HID 路径 / 语音内容

## 7. UI（Tauri + React）

- [ ] 连接页：设备选择、连接状态、重新连接
- [ ] 按键映射页：13 键布局、动作配置、手势配置
- [ ] 语音页：输出端点选择、测试音、语音输入方式（默认系统 Win+H）配置
- [ ] 诊断页：各项检查与修复入口
- [ ] 权限/帮助页：系统权限跳转、FAQ
- [ ] 系统托盘菜单
- [ ] 多语言（简体中文 / English）

## 8. 打包 / 签名 / 发布

- [ ] Tauri Bundle 配置
  - [ ] 安装器（NSIS）
  - [ ] 便携版 ZIP
  - [ ] SHA256SUMS.txt
- [ ] 签名方案
  - [ ] 免费自签 Authenticode
  - [ ] SmartScreen 提示文档
  - [ ] 可选：公共 CA / EV 签名评估
- [ ] 自动更新方案（WinSparkle / 自更新 / 手动下载）
- [ ] GitHub Release 自动化发布
- [ ] 卸载后配置/日志保留策略说明

## 9. 测试与真机验收

- [ ] Rust 单元测试：ATVV / ADPCM / 动作 / 配置
- [ ] Rust 集成测试：BLE contract / 音频 contract / 按键链
- [ ] 前端测试：Vitest + Playwright
- [ ] CI 全绿门禁
- [ ] 真机验收清单
  - [ ] RC003 配对 / 重连 / 断线恢复
  - [ ] 12 个普通键单次触发、无双触发
  - [ ] 双击 / 长按 / 按住重复
  - [ ] Win+H 系统语音输入出字（基础验收）
  - [ ] 麦克风键 → 豆包输入法出字（可选扩展）
  - [ ] 微信输入法出字（可选扩展）
  - [ ] 长时间语音（>60 秒）
  - [ ] 安装器 / 便携版 / 升级 / 卸载

## 10. 里程碑追踪

- [ ] M0：协议与基线复验完成
- [ ] M1：Rust 核心骨架 + Tauri 壳完成
- [ ] M2：按键链路真机通过
- [ ] M3：语音链路真机通过
- [ ] M4：产品化（安装器、签名、诊断、文档）
- [ ] M5：扩展（RC001、更多第三方 IME、统计、自动更新）