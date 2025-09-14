//! Exemple de plugin WASI: lit {"a":number,"b":number} et renvoie {"sum":a+b,"echo":...}
//! Build:
//!   rustup target add wasm32-wasi
//!   cargo build --target wasm32-wasi --release
//! Copie:
//!   mkdir -p .devit/plugins/echo_sum
//!   cp target/wasm32-wasi/release/echo_sum.wasm .devit/plugins/echo_sum/echo_sum.wasm
//!   cat > .devit/plugins/echo_sum/devit-plugin.toml <<'TOML'
//!   id = "echo_sum"
//!   name = "Echo Sum"
//!   wasm = "echo_sum.wasm"
//!   version = "0.1.0"
//!   allowed_dirs = []
//!   env = []
//!   TOML

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

#[derive(Deserialize)]
struct Req {
    a: f64,
    b: f64,
    #[allow(dead_code)]
    #[serde(default)]
    text: Option<String>,
}

#[derive(Serialize)]
struct Resp<'a> {
    sum: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    echo: Option<&'a str>,
}

fn main() {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).unwrap();
    let trimmed = buf.trim();
    let req: Req = match serde_json::from_str(trimmed) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("invalid json: {e}");
            std::process::exit(2);
        }
    };
    let resp = Resp { sum: req.a + req.b, echo: None };
    let out = serde_json::to_string(&resp).unwrap();
    let mut stdout = std::io::stdout();
    stdout.write_all(out.as_bytes()).unwrap();
    stdout.flush().unwrap();
}

