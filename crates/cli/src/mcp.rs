// MCP client minimal (stdio) — expérimental.
// - Handshake: ping -> version -> capabilities
// - Tool call démo: echo
// - Timeouts via DEVIT_TIMEOUT_SECS (par message)
//
// Ce module est consommé par le binaire `devit-mcp` uniquement.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub fn timeout_from_env() -> Duration {
    let secs = env::var("DEVIT_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(30);
    Duration::from_secs(secs)
}

/// Erreur sentinelle pour signaler un délai dépassé.
#[derive(Debug)]
pub struct TimeoutErr;
impl std::fmt::Display for TimeoutErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "timeout waiting line")
    }
}
impl std::error::Error for TimeoutErr {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    pub tools: Vec<String>,
}

#[derive(Debug)]
pub struct McpClient {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    per_msg_timeout: Duration,
}

impl McpClient {
    pub fn spawn_cmd(cmd: &str, per_msg_timeout: Duration) -> Result<Self> {
        // On passe par bash -lc pour supporter des pipelines/quoted.
        let mut child = Command::new("bash")
            .arg("-lc")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("spawn failed for: {cmd}"))?;

        let stdin = BufWriter::new(
            child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("child stdin missing"))?,
        );
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("child stdout missing"))?,
        );
        Ok(Self {
            child,
            stdin,
            stdout,
            per_msg_timeout,
        })
    }

    pub fn handshake(&mut self, client_version: &str) -> Result<Capabilities> {
        // 1) ping -> pong
        self.write_json(&json!({ "type": "ping" }))?;
        let pong = self.read_json_line_timeout()?;
        ensure_type(&pong, "pong")?;

        // 2) version exchange
        self.write_json(&json!({
            "type": "version",
            "payload": { "client": client_version }
        }))?;
        let ver = self.read_json_line_timeout()?;
        ensure_type(&ver, "version")?;

        // 3) capabilities
        self.write_json(&json!({ "type": "capabilities" }))?;
        let caps = self.read_json_line_timeout()?;
        ensure_type(&caps, "capabilities")?;
        let tools = caps
            .get("payload")
            .and_then(|p| p.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| anyhow!("invalid capabilities payload"))?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        Ok(Capabilities { tools })
    }

    pub fn tool_echo(&mut self, text: &str) -> Result<Value> {
        self.write_json(&json!({
            "type": "tool.call",
            "payload": { "name": "echo", "args": { "text": text } }
        }))?;
        let v = self.read_json_line_timeout()?;
        ensure_type(&v, "tool.result")?;
        Ok(v)
    }

    fn write_json(&mut self, v: &Value) -> Result<()> {
        let s = serde_json::to_string(v)?;
        self.stdin.write_all(s.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn read_json_line_timeout(&mut self) -> Result<Value> {
        let (tx, rx) = mpsc::sync_channel::<Result<String>>(1);
        let timeout = self.per_msg_timeout;
        let reader = &mut self.stdout;
        thread::scope(|s| {
            s.spawn(|| {
                let mut line = String::new();
                let r: Result<String> = match reader.read_line(&mut line) {
                    Ok(0) => Err(anyhow!("eof from server")),
                    Ok(_) => Ok(line),
                    Err(e) => Err(anyhow!(e)),
                };
                let _ = tx.send(r);
            });
            // scoped: wait for a line or timeout
            let line = match rx.recv_timeout(timeout) {
                Ok(res) => res?,
                Err(_e) => return Err(TimeoutErr.into()),
            };
            let v: Value =
                serde_json::from_str(&line).with_context(|| format!("invalid json: {line}"))?;
            Ok(v)
        })
    }
}

fn ensure_type(v: &Value, expected: &str) -> Result<()> {
    let t = v
        .get("type")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("missing type"))?;
    if t != expected {
        Err(anyhow!("unexpected type: {t}, want {expected}"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Fake {
        out: BufReader<Cursor<Vec<u8>>>,
        inp: Vec<u8>,
        timeout: Duration,
    }

    impl Fake {
        fn new(server_lines: &[&str]) -> Self {
            let joined = server_lines
                .iter()
                .map(|s| format!("{s}\n"))
                .collect::<String>()
                .into_bytes();
            Self {
                out: BufReader::new(Cursor::new(joined)),
                inp: Vec::new(),
                timeout: Duration::from_millis(200),
            }
        }

        fn read_json_line_timeout(&mut self) -> Result<Value> {
            let (tx, rx) = mpsc::sync_channel::<Result<String>>(1);
            let timeout = self.timeout;
            let reader = &mut self.out;
            thread::scope(|s| {
                s.spawn(|| {
                    // Simule un blocage si le buffer est vide pour forcer un timeout
                    let is_empty = reader.get_ref().get_ref().is_empty();
                    if is_empty {
                        // dormir plus longtemps que le timeout pour garantir le dépassement
                        std::thread::sleep(timeout + std::time::Duration::from_millis(50));
                        let _ = tx.send(Err(anyhow!("no data")));
                    } else {
                        let mut line = String::new();
                        let r: Result<String> = match reader.read_line(&mut line) {
                            Ok(0) => Err(anyhow!("eof from server")),
                            Ok(_) => Ok(line),
                            Err(e) => Err(anyhow!(e)),
                        };
                        let _ = tx.send(r);
                    }
                });
                let line = match rx.recv_timeout(timeout) {
                    Ok(res) => res?,
                    Err(_e) => return Err(TimeoutErr.into()),
                };
                let v: Value = serde_json::from_str(&line)?;
                Ok(v)
            })
        }

        fn write_json(&mut self, v: &Value) -> Result<()> {
            let s = serde_json::to_string(v)?;
            self.inp.extend_from_slice(s.as_bytes());
            self.inp.extend_from_slice(b"\n");
            Ok(())
        }
    }

    #[test]
    fn ensure_type_ok() {
        let v = json!({"type":"pong"});
        assert!(ensure_type(&v, "pong").is_ok());
    }

    #[test]
    fn ensure_type_bad() {
        let v = json!({"type":"nope"});
        assert!(ensure_type(&v, "pong").is_err());
    }

    #[test]
    fn handshake_flow_happy_path() {
        let mut fake = Fake::new(&[
            r#"{"type":"pong"}"#,
            r#"{"type":"version","payload":{"server":"mock"}}"#,
            r#"{"type":"capabilities","payload":{"tools":["echo"]}}"#,
        ]);
        fake.write_json(&json!({"type":"ping"})).unwrap();
        let pong = fake.read_json_line_timeout().unwrap();
        ensure_type(&pong, "pong").unwrap();
        fake.write_json(&json!({"type":"version","payload":{"client":"x"}}))
            .unwrap();
        let ver = fake.read_json_line_timeout().unwrap();
        ensure_type(&ver, "version").unwrap();
        fake.write_json(&json!({"type":"capabilities"})).unwrap();
        let caps = fake.read_json_line_timeout().unwrap();
        ensure_type(&caps, "capabilities").unwrap();
        let tools = caps["payload"]["tools"].as_array().unwrap();
        assert_eq!(tools[0], "echo");
    }

    #[test]
    fn read_timeout_errors() {
        let mut fake = Fake::new(&[]);
        let r = fake.read_json_line_timeout();
        let e = r.unwrap_err();
        assert!(e.downcast_ref::<TimeoutErr>().is_some());
    }
}
