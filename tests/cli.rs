// SPDX-License-Identifier: MIT
use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mf-cli"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn inspect_preserved_fixture_offline() {
    let output = run(&[
        "config",
        "show",
        "tests/fixtures/windows-state.json",
        "--json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["device"]["product_id"], 0xaf70);
    assert_eq!(result["state"]["mixes"].as_array().unwrap().len(), 3);
    assert_eq!(result["state"]["mixes"][0]["route"], "mix");
    assert_eq!(result["payload_hex"].as_str().unwrap().len(), 380);
}

#[test]
fn compare_two_snapshots_without_opening_hardware() {
    let output = run(&[
        "config",
        "diff",
        "tests/fixtures/windows-state.json",
        "--against",
        "tests/fixtures/windows-state.json",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["usb_write_count"], 0);
    assert!(
        result["unrestored_byte_offsets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn help_includes_new_commands_and_preserved_toggles() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for expected in [
        "status",
        "mixer",
        "route",
        "config",
        "MiniFuse 1/2",
        "inst1",
        "--backup-dir",
    ] {
        assert!(help.contains(expected));
    }
}
