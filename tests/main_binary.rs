use std::process::{Command, Stdio};

#[test]
fn main_binary_reports_connection_closed_when_stdio_is_unavailable() {
    let output = Command::new(env!("CARGO_BIN_EXE_zeuxis"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .expect("run zeuxis binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ConnectionClosed") || stderr.contains("initialized request"),
        "unexpected stderr: {stderr}"
    );
}
