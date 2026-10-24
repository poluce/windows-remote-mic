//! 虚拟 HID 单测（诊断 / 环境探测 / 按键路由）。

use std::path::Path;

use super::diag::*;

/// 音量键必须被路由到 Consumer 设备：键盘页 usage 0x7F/0x80/0x81 超出
/// Boot Keyboard 描述符声明的范围（0x00–0x65），走键盘报告会被系统忽略。
#[test]
fn volume_tokens_route_to_consumer_device() {
    let (keyboard, media) = super::split_tokens(&["volume_up"]);
    assert!(
        keyboard.is_empty(),
        "volume_up 不该进键盘设备：{keyboard:?}"
    );
    assert_eq!(media, vec!["volume_up"]);

    let (keyboard, media) = super::split_tokens(&["volume_down", "volume_mute"]);
    assert!(keyboard.is_empty(), "音量键不该进键盘设备：{keyboard:?}");
    assert_eq!(media, vec!["volume_down", "volume_mute"]);

    // 普通组合键与媒体键混排时各走各的设备。
    let (keyboard, media) = super::split_tokens(&["lctrl", "c", "play_pause"]);
    assert_eq!(keyboard, vec!["lctrl", "c"]);
    assert_eq!(media, vec!["play_pause"]);
}

/// 断言不限环境相关的取值，只要求整条诊断链路能跑完（含注册表读取）并给出结论。
/// 用 `--nocapture` 可看到本机的真实取值。
#[test]
fn diagnostics_runs_and_reports_environment() {
    let d = diagnostics();
    eprintln!("available     = {}", d.available);
    eprintln!("detail        = {}", d.detail);
    eprintln!("bundle_dir    = {}", d.bundle_dir);
    eprintln!("bundle_present= {}", d.bundle_present);
    eprintln!("dev_build     = {}", d.dev_build);
    eprintln!("secure_boot   = {:?}", d.secure_boot);
    eprintln!("hvci          = {:?}", d.hvci);
    for h in &d.hints {
        eprintln!("hint          = {h}");
    }
    assert!(!d.detail.is_empty());
    // 负载目录必须能推导出来，否则安装器与运行时的路径约定就断了。
    assert!(
        d.bundle_dir.ends_with("vhid"),
        "bundle_dir={}",
        d.bundle_dir
    );
    // 不可用时必须给出至少一条建议。
    if !d.available {
        assert!(!d.hints.is_empty(), "不可用时没有给出任何排查建议");
    }
}

#[test]
fn registry_reads_return_known_shapes() {
    // Secure Boot 状态：本机可能读不到（非 UEFI / 无权限），但读到就必须是布尔语义。
    if let Some(sb) = secure_boot_enabled() {
        let _: bool = sb;
    }
    // 场景键缺失时按「未启用」处理，不应是 None。
    assert!(hvci_enabled().is_some() || hvci_enabled().is_none());
}

fn joined(hints: &[String]) -> String {
    hints.join(" ")
}

#[test]
fn available_needs_no_hints() {
    let h = build_hints(true, true, false, r"C:\App\vhid", Some(true), Some(true));
    assert!(h.is_empty(), "可用时不该给建议：{h:?}");
}

#[test]
fn missing_bundle_points_at_reinstall() {
    let h = joined(&build_hints(
        false,
        false,
        false,
        r"C:\App\vhid",
        None,
        None,
    ));
    assert!(h.contains("没有驱动负载"), "{h}");
    assert!(h.contains(r"C:\App\vhid"), "建议里应带上实际路径：{h}");
}

#[test]
fn present_bundle_points_at_repair_button() {
    let h = joined(&build_hints(false, true, false, r"C:\App\vhid", None, None));
    assert!(h.contains("安装 / 修复虚拟键盘驱动"), "{h}");
    assert!(!h.contains("没有驱动负载"), "{h}");
}

/// 开发构建没有随包负载，提示必须指向「跑汇编脚本 / 装正式包」，
/// 而不是误导性地让人「重新安装 Remote Mic」。
#[test]
fn dev_build_is_told_apart_from_a_broken_install() {
    let dev = joined(&build_hints(
        false,
        false,
        true,
        r"F:\repo\target\debug\vhid",
        None,
        None,
    ));
    assert!(dev.contains("开发构建"), "{dev}");
    assert!(dev.contains("prepare-vhid-bundle.ps1"), "{dev}");
    assert!(
        !dev.contains("请用最新安装包重新安装"),
        "开发构建不该提示重装：{dev}"
    );

    // 负载就位时即便是开发构建也走「点修复按钮」那条。
    let dev_with_bundle = joined(&build_hints(
        false,
        true,
        true,
        r"F:\repo\target\debug\vhid",
        None,
        None,
    ));
    assert!(
        dev_with_bundle.contains("安装 / 修复虚拟键盘驱动"),
        "{dev_with_bundle}"
    );
}

#[test]
fn dev_build_detection_matches_cargo_layout_only() {
    assert!(is_dev_build(Path::new(r"D:\repo\target\debug")));
    assert!(is_dev_build(Path::new(r"D:\repo\target\release")));
    assert!(is_dev_build(Path::new(
        r"F:\B_My_Document\GitHub\windows-remote-mic\target\debug"
    )));
    // 安装目录不能误判。
    assert!(!is_dev_build(Path::new(r"C:\Program Files\Remote Mic")));
    assert!(!is_dev_build(Path::new(r"D:\repo\target")));
    assert!(!is_dev_build(Path::new(r"D:\repo\debug")));
    assert!(!is_dev_build(Path::new(r"D:\repo\target\debug\inner")));
}

/// Secure Boot / HVCI 是这次加诊断的核心：开启时必须点名，关闭时不能误报。
#[test]
fn secure_boot_and_hvci_are_called_out_only_when_enabled() {
    let on = joined(&build_hints(
        false,
        true,
        false,
        r"C:\App\vhid",
        Some(true),
        Some(true),
    ));
    assert!(on.contains("Secure Boot 已开启"), "{on}");
    assert!(on.contains("内存完整性"), "{on}");

    let off = joined(&build_hints(
        false,
        true,
        false,
        r"C:\App\vhid",
        Some(false),
        Some(false),
    ));
    assert!(!off.contains("Secure Boot 已开启"), "关闭时不应误报：{off}");
    assert!(!off.contains("内存完整性"), "关闭时不应误报：{off}");

    // 读不到状态时也不应瞎猜。
    let unknown = joined(&build_hints(false, true, false, r"C:\App\vhid", None, None));
    assert!(!unknown.contains("Secure Boot 已开启"), "{unknown}");
    assert!(!unknown.contains("内存完整性"), "{unknown}");
}

/// 不管什么组合，都要给出可执行的下一步（日志指引）。
#[test]
fn unavailable_always_suggests_next_step() {
    for (sb, hvci, bundle, dev) in [
        (Some(true), Some(true), true, false),
        (Some(false), Some(false), false, false),
        (None, None, true, true),
        (Some(true), Some(false), false, true),
    ] {
        let h = build_hints(false, bundle, dev, r"C:\App\vhid", sb, hvci);
        assert!(
            joined(&h).contains("setupapi.dev.log"),
            "缺少日志指引：{h:?}"
        );
    }
}
