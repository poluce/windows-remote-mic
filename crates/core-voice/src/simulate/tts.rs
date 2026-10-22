//! Windows SAPI 中文语音合成（PowerShell + System.Speech）。

use std::io::Write;
use std::path::Path;
use std::process::Command;

/// PowerShell TTS 脚本：将一段中文短语合成为 WAV 文件。
/// 刻意保持纯 ASCII；朗读文本通过环境变量传入，
/// 因此脚本文件中不包含任何非 ASCII 字符。
const TTS_PS1: &str = r#"param([string]$OutputPath)
$text = $env:REMOTE_MIC_TTS_TEXT
Add-Type -AssemblyName System.Speech
$synthesizer = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voice = $synthesizer.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like 'zh-*' } | Select-Object -First 1
if ($voice) { $synthesizer.SelectVoice($voice.VoiceInfo.Name) }
$synthesizer.SetOutputToWaveFile($OutputPath)
$synthesizer.Speak($text)
$synthesizer.Dispose()
"#;
/// 使用 Windows SAPI TTS 将一段简短中文短语合成为 WAV 文件。
pub(super) fn synthesize_chinese_speech(output_path: &Path) -> Result<(), String> {
    let script_path =
        std::env::temp_dir().join(format!("remote_mic_tts_{}.ps1", std::process::id()));
    {
        let mut file =
            std::fs::File::create(&script_path).map_err(|e| format!("写入 TTS 脚本失败：{e}"))?;
        file.write_all(TTS_PS1.as_bytes())
            .map_err(|e| format!("写入 TTS 脚本失败：{e}"))?;
    }

    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script_path)
        .arg("-OutputPath")
        .arg(output_path)
        .env("REMOTE_MIC_TTS_TEXT", "这是一段中文语音测试")
        .output()
        .map_err(|e| format!("运行 TTS 脚本失败：{e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("TTS 合成失败：{stderr}"));
    }
    if !output_path.exists() {
        return Err("TTS 未生成 WAV 文件".to_string());
    }

    Ok(())
}
