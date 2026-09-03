# ATVV 协议（Android TV Voice over BLE）

> 对象：RC003 / 小米蓝牙语音遥控器 2 Pro 的语音传输协议。
> 可信度：✅ 实证 / ⚠️ 社区/推断 / ❓ 未验证。

---

## 1. GATT 服务与特征

| 名称 | UUID | 方向 | 用途 |
| --- | --- | --- | --- |
| ATVV Service | `AB5E0001-5A21-4F05-BC7D-AF01F617B664` | — | 语音服务 |
| TX | `AB5E0002-5A21-4F05-BC7D-AF01F617B664` | Host → Device | 主机命令（写） |
| Audio (RX) | `AB5E0003-5A21-4F05-BC7D-AF01F617B664` | Device → Host | 音频通知 |
| Control (CTL) | `AB5E0004-5A21-4F05-BC7D-AF01F617B664` | Device → Host | 控制通知 |

---

## 2. Host → Device（TX）

| 命令 | 载荷 | 说明 |
| --- | --- | --- |
| GET_CAPABILITIES_V10 | `0A 01 00 00 03 03` | 能力查询 |
| MIC_OPEN | `0C ...` | 打开麦克风；不同设备载荷长度不同 |
| MIC_CLOSE | `0D ...` | 关闭麦克风；载荷影响是否真正停止 |
| MIC_EXTEND | v1.0+ 长语音续期 | 部分设备用于 keepalive，静默续期 |

---

## 3. Device → Host（Control 通知，首字节 opcode）

| 事件 | opcode | 说明 |
| --- | --- | --- |
| AUDIO_STOP | `0x00` | 语音流结束 |
| AUDIO_START | `0x04` | 语音流开始 |
| MIC_BUTTON | `0x08` | 麦克风键按下事件（RC003 未观察到经此通道发送） |
| AUDIO_SYNC | `0x0A` | 解码器同步（部分设备） |
| CAPS | `0x0B` | 能力协商响应 |
| MIC_OPEN_ERROR | `0x0C` | MIC_OPEN 失败（某些设备） |

> 社区也称 `0x08` 为 START_SEARCH、`0x0B` 为 GET_CAPS_RESP / CAPS_RESP。

---

## 4. CAPS 解析

payload 首字节 `0x0B`：

```text
byte0: opcode 0x0B
byte1-2: version (big-endian)
byte3: codecs (v1.0+)
byte4: interaction (v1.0+)
byte5-6: frame_size (big-endian; 0 -> 120)
byte7+: 扩展/旧版字段
```

- v1.0：若 `codecs == 0` 且 `byte4 & 0x03 != 0`，则 `codecs = byte4`、`interaction = 0x03`
- 旧版：`codecs = byte4`
- `selected_codec = 0x02` → 16 kHz，否则 8 kHz

❓ interaction 各 bit 含义、扩展字段长度未确认。

---

## 5. 音频格式

| 项 | 值 | 说明 |
| --- | --- | --- |
| 编码 | IMA/DVI ADPCM 4-bit | |
| 采样率 | 16 kHz（RC003） | 部分 ATVV 设备为 8 kHz |
| 采样顺序 | 高半字节优先 | |
| 每字节采样数 | 2 | |
| 默认帧长 | 120 字节（RC003） | 以 CAPS 为准 |
| 帧长变体 | 120 / 126（带同步头）/ 134 / 160 | 不同设备/固件有差异 |

### RC003 相关帧格式

| 帧格式 | 采样率 | 说明 |
| --- | --- | --- |
| 120B 裸 ADPCM | 16 kHz | RC003 常见格式 |
| 126B 带同步头 | 16 kHz | 有社区实现对 RC003 兼容；含同步/AUDIO_SYNC 相关头 |

---

## 6. 会话行为

| 行为 | 说明 |
| --- | --- |
| AUDIO_START | 开始语音流；解码器/同步状态重置 |
| AUDIO_STOP | 结束语音流 |
| 长语音 | 硬件有 Audio Transfer Timeout（常见 15–60s），需周期性 keepalive（MIC_EXTEND / MIC_OPEN） |

❓ RC003 的 MIC_OPEN/MIC_CLOSE 主动发送时序尚未完全验证。

---

## 7. 未知 / 待确认

| 项 | 状态 |
| --- | --- |
| MIC_OPEN / MIC_CLOSE 在 RC003 上的真实时序与载荷 | ❓ |
| AUDIO_SYNC 完整结构 | ❓ |
| interaction 含义 | ❓ |
| RC003 是否出现 126B 带同步头帧固件 | ❓ |
| RC003 长按超过硬件超时后的行为 | ❓ |
