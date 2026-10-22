//! PCM WAV 解析：转成 `(sample_rate, mono i16)`。

/// 解析标准 PCM WAV，返回 `(sample_rate, mono i16 samples)`。
pub(super) fn load_wav_pcm(data: &[u8]) -> Result<(u32, Vec<i16>), String> {
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err("不是有效的 WAV 文件".to_string());
    }

    let mut sample_rate = 0u32;
    let mut channels = 0u16;
    let mut bits_per_sample = 0u16;
    let mut raw_data: Option<&[u8]> = None;

    let mut pos = 12usize;
    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size =
            u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
                as usize;
        let body = pos + 8;
        if body + chunk_size > data.len() {
            break;
        }

        match chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    return Err("WAV fmt 块不完整".to_string());
                }
                let audio_format = u16::from_le_bytes([data[body], data[body + 1]]);
                if audio_format != 1 {
                    return Err("仅支持 PCM WAV".to_string());
                }
                channels = u16::from_le_bytes([data[body + 2], data[body + 3]]);
                sample_rate = u32::from_le_bytes([
                    data[body + 4],
                    data[body + 5],
                    data[body + 6],
                    data[body + 7],
                ]);
                bits_per_sample = u16::from_le_bytes([data[body + 14], data[body + 15]]);
            }
            b"data" => {
                raw_data = Some(&data[body..body + chunk_size]);
            }
            _ => {}
        }

        pos = body + chunk_size + (chunk_size % 2);
    }

    if sample_rate == 0 || channels == 0 || bits_per_sample == 0 {
        return Err("WAV 缺少必要的格式信息".to_string());
    }
    let raw_data = raw_data.ok_or_else(|| "WAV 缺少 data 块".to_string())?;

    let mut pcm = Vec::with_capacity(raw_data.len() / 2);
    match bits_per_sample {
        16 => {
            for pair in raw_data.as_chunks::<2>().0 {
                pcm.push(i16::from_le_bytes(*pair));
            }
        }
        8 => {
            for &byte in raw_data {
                pcm.push(((i16::from(byte)) - 128) << 8);
            }
        }
        _ => return Err(format!("不支持的位深：{bits_per_sample}")),
    }

    if channels > 1 {
        let frame_count = pcm.len() / channels as usize;
        let mut mono = Vec::with_capacity(frame_count);
        for frame in pcm.chunks_exact(channels as usize) {
            let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
            mono.push((sum / i32::from(channels)) as i16);
        }
        pcm = mono;
    }

    if pcm.is_empty() {
        return Err("WAV 中没有音频样本".to_string());
    }

    Ok((sample_rate, pcm))
}
