use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn spawn_server() -> std::process::Child {
    let exe = env!("CARGO_BIN_EXE_devit-mcpd");
    Command::new(exe)
        .arg("--yes")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn mcpd")
}

fn roundtrip(child: &mut std::process::Child, line: &str) -> String {
    let mut sin = child.stdin.as_ref().unwrap();
    writeln!(sin, "{}", line).unwrap();
    let mut reader = BufReader::new(child.stdout.as_mut().unwrap());
    let mut out = String::new();
    reader.read_line(&mut out).unwrap();
    out
}

#[test]
fn devit_tool_call_schema_errors() {
    let mut child = spawn_server();
    // missing tool
    let out = roundtrip(
        &mut child,
        r#"{"type":"tool.call","payload":{"name":"devit.tool_call","args":{"args":{}}}}"#,
    );
    assert!(out.contains("\"schema_error\":true") && out.contains("payload.tool"));
    // wrong type for args
    let out = roundtrip(
        &mut child,
        r#"{"type":"tool.call","payload":{"name":"devit.tool_call","args":{"tool":"echo","args":1}}}"#,
    );
    assert!(
        out.contains("\"schema_error\":true")
            && out.contains("payload.args")
            && out.contains("type_mismatch")
    );
    let _ = child.kill();
}

#[test]
fn plugin_invoke_schema_errors() {
    let mut child = spawn_server();
    // missing id
    let out = roundtrip(
        &mut child,
        r#"{"type":"tool.call","payload":{"name":"plugin.invoke","args":{"payload":{}}}}"#,
    );
    assert!(
        out.contains("\"schema_error\":true")
            && out.contains("payload.id")
            && out.contains("missing")
    );
    // wrong type for payload
    std::thread::sleep(std::time::Duration::from_millis(300));
    let out = roundtrip(
        &mut child,
        r#"{"type":"tool.call","payload":{"name":"plugin.invoke","args":{"id":"x","payload":1}}}"#,
    );
    assert!(
        out.contains("\"schema_error\":true")
            && out.contains("payload.payload")
            && out.contains("type_mismatch"),
        "unexpected output: {}",
        out
    );
    let _ = child.kill();
}
