//! Standalone test: verify that the app can really switch the default
//! microphone to CABLE Output and restore it afterwards.
//!
//! Run on Windows:
//!   cargo run -p core-audio --example check_default_mic

use core_audio::default_device::DefaultInputGuard;
use core_audio::endpoint::default_input_name;
use std::thread;
use std::time::Duration;

fn main() {
    println!("=== Default microphone switch test ===");
    println!("before: {:?}", default_input_name());

    println!("switching to CABLE Output ...");
    let guard = DefaultInputGuard::switch_to_cable_output().expect("switch to CABLE Output failed");

    // Give Windows a moment to settle, then read the actual default input.
    thread::sleep(Duration::from_millis(500));
    println!("after : {:?}", default_input_name());

    let switched_ok = default_input_name()
        .map(|name| name.to_lowercase().contains("cable output"))
        .unwrap_or(false);
    println!("switch_ok: {}", switched_ok);

    drop(guard);
    thread::sleep(Duration::from_millis(500));
    println!("restored: {:?}", default_input_name());
    println!("=== done ===");
}