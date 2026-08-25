# Xiaomi Vibe（Rust）

把 **小米蓝牙语音遥控器 2 Pro / RC003** 做成后台输入设备：不占终端、跨 macOS / Linux / Windows 11。

USB-C **只能充电**。按键走蓝牙 HID（VID `0x2717` PID `0x32B8`），语音走 ATVV GATT。

## 运行（图形在浏览器里）

```bash
cargo run --release
```

会在后台听遥控器，并打开 `http://127.0.0.1:17890`：

1. **搜索/配对向导**（按系统写好了步骤）
2. **按键映射**（Cursor 快捷键）
3. **三维遥控**：方向键 = XoY，音量± = Z

关掉终端窗口前请用 `nohup` 或系统服务启动，浏览器只是控制台。

```bash
# macOS / Linux 后台
nohup ./target/release/xiaomi-vibe >/tmp/xiaomi-vibe.log 2>&1 &
```

Windows 发布版不弹控制台（`windows_subsystem = "windows"`）。

## 配对

遥控器 **同时按住 主页 + 菜单** 直到灯闪。

| 系统 | 去哪连 |
| --- | --- |
| macOS | 系统设置 → 蓝牙 → 小米蓝牙语音遥控器 |
| Windows 11 | 设置 → 蓝牙和其他设备 → 添加设备 |
| Linux | `bluetoothctl scan on` / `pair`，用户加入 `input` 组 |

页面点「扫描附近的遥控器」可确认广播。HID 变绿后按键生效。

## 三维遥控（机器人 / 无人机 / 机械臂 / 3D 打印机）

模式选 **三维遥控 XYZ**。按住方向键/音量键时，向 `127.0.0.1:9870`（可改）发 UDP。

JSON（默认）：

```json
{"schema":"xiaomi-vibe-teleop/v1","device":"rc003","vx":0.0,"vy":1.0,"vz":0.0,"protocol":"json","ts_ms":0}
```

| 遥控器 | 轴 |
| --- | --- |
| 右 / 左 | +X / −X |
| 上 / 下 | +Y / −Y |
| 音量+ / 音量− | +Z / −Z |

G-code 协议会发相对点动：

```
G91
G0 X0.000 Y1.000 Z0.000 F600
```

接收端示例：

```bash
python3 -c "import socket;s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.bind(('0.0.0.0',9870));
while True:
 d,a=s.recvfrom(4096); print(a, d.decode())"
```

把 `vx,vy,vz` 接到 ROS Twist、MAVLink 速度、机械臂笛卡尔增量或打印机 jog 即可。

## Cursor 映射

默认：OK=Enter，主页=Meta+L，菜单=Meta+I，返回=Esc。可在网页里改 `control+c` 这种组合。

语音键走 ATVV。ASR 命令可填（`{wav}` 会换成录音文件），例如：

```text
whisper-cli -f {wav} --language zh -nt
```

留空则只记录采样，不自动出字。macOS 上仍可用仓库里原来的 Python/Swift 听写桥。

## 配置

`~/Library/Application Support/xiaomi-vibe/config.json`（Linux: `~/.config/xiaomi-vibe/`，Windows: `%APPDATA%\xiaomi-vibe\`）

## 依赖

- Rust 1.80+
- 系统蓝牙 + HID 权限（macOS：输入监控）
- Linux：`libudev`、BlueZ

`xiaomi_vibe/` 下的 Python 原型仍可参考，主程序是本 Rust crate。
