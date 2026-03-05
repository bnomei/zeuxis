use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde_json::Value;

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

#[test]
fn main_binary_keeps_mcp_stdout_json_clean_during_tools_handshake() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zeuxis"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zeuxis binary");

    let test_result = (|| -> Result<(), String> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "child stdin unavailable".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout unavailable".to_owned())?;
        let mut lines = BufReader::new(stdout).lines();

        let initialize = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "integration-test", "version": "0" }
            }
        });
        writeln!(stdin, "{initialize}").map_err(|error| format!("write initialize: {error}"))?;
        stdin
            .flush()
            .map_err(|error| format!("flush initialize: {error}"))?;

        let init_line = lines
            .next()
            .ok_or_else(|| "missing initialize response line".to_owned())?
            .map_err(|error| format!("read initialize response: {error}"))?;
        let init_json: Value = serde_json::from_str(&init_line)
            .map_err(|error| format!("initialize response not JSON: {error}; line={init_line}"))?;
        assert_eq!(init_json.get("id").and_then(Value::as_i64), Some(1));
        assert!(
            init_json
                .get("result")
                .and_then(|result| result.get("capabilities"))
                .and_then(|capabilities| capabilities.get("tools"))
                .is_some()
        );

        let initialized = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        writeln!(stdin, "{initialized}")
            .map_err(|error| format!("write initialized notification: {error}"))?;

        let tools_list = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        writeln!(stdin, "{tools_list}").map_err(|error| format!("write tools/list: {error}"))?;
        stdin
            .flush()
            .map_err(|error| format!("flush tools/list: {error}"))?;

        let tools_line = lines
            .next()
            .ok_or_else(|| "missing tools/list response line".to_owned())?
            .map_err(|error| format!("read tools/list response: {error}"))?;
        let tools_json: Value = serde_json::from_str(&tools_line)
            .map_err(|error| format!("tools/list response not JSON: {error}; line={tools_line}"))?;
        assert_eq!(tools_json.get("id").and_then(Value::as_i64), Some(2));
        let tool_count = tools_json
            .get("result")
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .map_or(0, |tools| tools.len());
        assert!(
            tool_count > 0,
            "expected tools/list to return at least one tool"
        );

        Ok(())
    })();

    let _ = child.kill();
    let _ = child.wait();

    test_result.unwrap_or_else(|error| panic!("{error}"));
}
