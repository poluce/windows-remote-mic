# ATVV 协议事实表（社区整理）

> 用途：作为我们 Windows 实现的协议“唯一事实来源”。
> 这些是互操作事实（UUID、opcode、ADPCM 表），不是任何项目的创意代码。

## GATT

| 用途 | UUID |
| --- | --- |
| Service | `AB5E0001-5A21-4F05-BC7D-AF01F617B664` |
| Transmit (TX) | `AB5E0002-5A21-4F05-BC7D-AF01F617B664` |
| Audio | `AB5E0003-5A21-4F05-BC7D-AF01F617B664` |
| Control | `AB5E0004-5A21-4F05-BC7D-AF01F617B664` |

## Host → Device（写入 TX 特征）

| 命令 | 字节 |
| --- | --- |
| GET_CAPABILITIES_V10 | `0A 01 00 00 03 03` |
| MIC_OPEN（v1.0+） | `0C 00` |
| MIC_OPEN（旧版） | `0C 00 00` |
| MIC_CLOSE（v1.0+） | `0D <session_id>` |
| MIC_CLOSE（旧版） | `0D` |

## Device → Host（Control 特征通知，opcode = 首字节）

| 事件 | opcode |
| --- | --- |
| AUDIO_STOP | `0x00` |
| AUDIO_START | `0x04` |
| MIC_BUTTON | `0x08` |
| AUDIO_SYNC | `0x0A` |
| CAPS | `0x0B` |

## CAPS 解析（payload[0]=0x0B）

- version = `(payload[1] << 8) | payload[2]`
- v1.0+：codecs=payload[3]，interaction=payload[4]；特殊：codecs=0 且有特殊位时用 payload[4] 回退
- 旧版：codecs=payload[4]
- frame_size = `(payload[5] << 8) | payload[6]`，0 则用 120
- selected_codec = `0x02` → 16 kHz；否则 8 kHz
- 本实现只接受 **16 kHz**

## 音频

- 采样率：16 kHz
- 编码：IMA/DVI ADPCM，4-bit，**高半字节优先**
- 每字节 → 2 个 PCM sample
- 默认帧长：120 字节
- 同步包 `AUDIO_SYNC`：payload[4:6] 大端 signed predictor，payload[6] step_index

## 会话行为

- AUDIO_START：重置 decoder / framer / sync
- AUDIO_STOP：mic 关闭，清 framer；之后 0.3s 内的音频视为尾部丢弃
- MIC_BUTTON：遥控器物理麦克风键按下事件
- 停止排空：约 0.12s

## 参考社区来源

- `b0o/ATVVoice`
- `nijez/open-voice-bridge`
- `xxb26553663-star/remote-bridge-hub`
- `81199000/mi-remote-mapper`

> 真机验收后如与这些字节有差异，以真机日志为准并更新本表。
