use serde::Serialize;

use core_mapping::gesture::GestureDetector;

/// One log file entry.
#[derive(Serialize)]
pub struct LogFileInfo {
    name: String,
    path: String,
    size: u64,
}

/// One self-test item with a PASS / FAIL / SKIP verdict.
#[derive(Serialize)]
pub struct SelfTestItem {
    name: String,
    status: String,
    detail: String,
}

/// Decode a batch of ATVV audio bytes through the voice engine (self-test).
#[tauri::command]
pub fn decode_atvv_preview(bytes: Vec<u8>) -> core_voice::VoiceChunk {
    let mut engine = core_voice::VoiceEngine::new();
    let _ = engine.on_control(core_atvv::protocol::ControlEvent::StreamStart);
    engine.feed(&bytes)
}

fn list_logs_in_dir(dir: &std::path::Path, prefix: &str, out: &mut Vec<LogFileInfo>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                out.push(LogFileInfo {
                    name: format!("{prefix}{}", entry.file_name().to_string_lossy()),
                    path: path.display().to_string(),
                    size,
                });
            }
        }
    }
}

/// List local log and capture files (config/logs/captures).
#[tauri::command]
pub fn list_log_files() -> Vec<LogFileInfo> {
    let base = std::env::var("LOCALAPPDATA")
        .map(|b| std::path::PathBuf::from(b).join("RemoteMic/RC003"))
        .unwrap_or_default();
    let mut out = Vec::new();
    list_logs_in_dir(&base, "", &mut out);
    list_logs_in_dir(&base.join("logs"), "logs/", &mut out);
    list_logs_in_dir(&base.join("captures"), "captures/", &mut out);
    out
}

/// Read a text log file (truncated to a safe size for the UI).
#[tauri::command]
pub fn read_log_file(path: String) -> Result<String, String> {
    let data = std::fs::read(&path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&data).to_string();
    Ok(limit_text(&text, 50_000))
}

fn limit_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("...（已截断）\n{}", &text[start..])
}

/// Run a hardware-independent capability self-test (Windows does the audio part).
#[tauri::command]
pub fn run_self_test() -> Vec<SelfTestItem> {
    let mut items = Vec::new();

    // 1) Audio endpoints
    match core_audio::endpoint::list_output_endpoints() {
        Ok(list) if !list.is_empty() => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "pass".into(),
            detail: format!("发现 {} 个输出端点", list.len()),
        }),
        Ok(_) => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "fail".into(),
            detail: "未发现输出端点".into(),
        }),
        Err(e) => items.push(SelfTestItem {
            name: "音频端点枚举".into(),
            status: "fail".into(),
            detail: e.to_string(),
        }),
    }

    // 2) Voice decode preview (synthetic ATVV bytes)
    {
        let mut engine = core_voice::VoiceEngine::new();
        let _ = engine.on_control(core_atvv::protocol::ControlEvent::StreamStart);
        let chunk = engine.feed(&[0x55; 120]);
        if chunk.output_samples > 0 && chunk.pcm_samples > 0 {
            items.push(SelfTestItem {
                name: "ATVV→ADPCM→输出帧".into(),
                status: "pass".into(),
                detail: format!("PCM {}，输出帧 {}", chunk.pcm_samples, chunk.output_samples),
            });
        } else {
            items.push(SelfTestItem {
                name: "ATVV→ADPCM→输出帧".into(),
                status: "fail".into(),
                detail: "解码输出为空".into(),
            });
        }
    }

    // 3) Gesture detection
    {
        let mut d = GestureDetector::new();
        d.press(0);
        let fired = d.release(600);
        use core_mapping::gesture::FeedOutcome;
        use core_mapping::Trigger;
        match fired {
            FeedOutcome::Fire(ev) if ev.trigger == Trigger::LongPress => {
                items.push(SelfTestItem {
                    name: "长按手势识别".into(),
                    status: "pass".into(),
                    detail: "550ms 长按被正确识别".into(),
                });
            }
            other => items.push(SelfTestItem {
                name: "长按手势识别".into(),
                status: "fail".into(),
                detail: format!("期望 LongPress，实际 {:?}", other),
            }),
        }
    }

    // 4) Local stats write/read
    {
        let dir = std::env::temp_dir().join("remote-mic-self-test");
        let _ = std::fs::remove_dir_all(&dir);
        if let Ok(store) = core_stats::StatsStore::new(&dir) {
            let ok = store.record_key("self_test").is_ok()
                && store
                    .load()
                    .map(|m| {
                        m.values()
                            .any(|d| d.key_presses.get("self_test").copied().unwrap_or(0) > 0)
                    })
                    .unwrap_or(false);
            items.push(SelfTestItem {
                name: "本地统计读写".into(),
                status: if ok { "pass" } else { "fail" }.into(),
                detail: if ok { "统计写读一致".into() } else { "统计读写失败".into() },
            });
        } else {
            items.push(SelfTestItem {
                name: "本地统计读写".into(),
                status: "fail".into(),
                detail: "无法创建统计目录".into(),
            });
        }
    }

    // 5) Test tone playback (Windows only)
    #[cfg(target_os = "windows")]
    {
        match core_audio::playback::play_test_tone(None) {
            Ok(()) => items.push(SelfTestItem {
                name: "测试音播放（CABLE 验证）".into(),
                status: "pass".into(),
                detail: "已写入默认输出端点约 1 秒".into(),
            }),
            Err(e) => items.push(SelfTestItem {
                name: "测试音播放（CABLE 验证）".into(),
                status: "fail".into(),
                detail: e.to_string(),
            }),
        }
    }
    #[cfg(not(target_os = "windows"))]
    items.push(SelfTestItem {
        name: "测试音播放（CABLE 验证）".into(),
        status: "skip".into(),
        detail: "仅限 Windows".into(),
    });

    items
}

