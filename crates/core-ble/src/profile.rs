//! RC003 identity + name matching (pure logic, unit-testable).

/// Bluetooth names the RC003 / Remote 2 Pro is paired as.
pub const RC003_BLUETOOTH_NAMES: [&str; 3] = [
    "MI RC",
    "Xiaomi Bluetooth Remote 2 Pro",
    "小米蓝牙语音遥控器",
];

/// Normalize a name for comparison (trim + case-insensitive).
pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Whether a device name matches the RC003 we target.
pub fn matches_rc003(name: &str) -> bool {
    let name = normalize_name(name);
    RC003_BLUETOOTH_NAMES
        .iter()
        .map(|n| normalize_name(n))
        .any(|n| n == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_names_case_insensitive() {
        assert!(matches_rc003("mi rc"));
        assert!(matches_rc003("MI RC"));
        assert!(matches_rc003("Xiaomi Bluetooth Remote 2 Pro"));
        assert!(matches_rc003(" 小米蓝牙语音遥控器 "));
    }

    #[test]
    fn ignores_other_devices() {
        assert!(!matches_rc003("Xiaomi Soundbar"));
        assert!(!matches_rc003("RC003"));
        assert!(!matches_rc003("USB Audio"));
    }
}
