# devit-plugin-echo-sum (WASM/WASI example)

Minimal WASI plugin that reads `{\"a\":number, \"b\":number}` on stdin and outputs `{\"sum\": a+b}` on stdout.

## Requirements
- Rust target: `wasm32-wasi`
- Wasmtime runtime in PATH: `wasmtime` (https://wasmtime.dev)

## Build
```
rustup target add wasm32-wasi
cargo build -p devit-plugin-echo-sum --target wasm32-wasi --release
```
The artifact will be at `target/wasm32-wasi/release/echo_sum.wasm`.

## Install to local registry
```
mkdir -p .devit/plugins/echo_sum
cp target/wasm32-wasi/release/echo_sum.wasm .devit/plugins/echo_sum/
cat > .devit/plugins/echo_sum/devit-plugin.toml <<'TOML'
id = "echo_sum"
name = "Echo Sum"
wasm = "echo_sum.wasm"
version = "0.1.0"
allowed_dirs = []
env = []
TOML
```

## Invoke (JSON I/O)
Use the experimental plugin runner:
```
echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --id echo_sum
```
Expected output:
```
{"sum":3.0}
```

## Notes
- Timeout is controlled by `DEVIT_TIMEOUT_SECS` (default 30). On timeout, exit code is 124.
- For direct manifest invocation:
```
echo '{"a":1,"b":2}' | cargo run -p devit-cli --features experimental --bin devit-plugin -- invoke --manifest .devit/plugins/echo_sum/devit-plugin.toml
```
