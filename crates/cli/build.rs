fn main() {
    use std::process::Command;
    fn run(cmd: &mut Command) -> Option<String> {
        let out = cmd.output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
    let describe = run(Command::new("git").args([
        "describe",
        "--tags",
        "--always",
        "--dirty=-m",
    ]))
    .unwrap_or_else(|| "unknown".to_string());
    let sha = run(Command::new("git").args(["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=DEVIT_GIT_DESCRIBE={}", describe);
    println!("cargo:rustc-env=DEVIT_GIT_SHA={}", sha);
}

