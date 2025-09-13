use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn write_cfg(dir: &std::path::Path, approval: &str) {
    let cfg = format!(
        "[backend]\nkind='openai_like'\nbase_url=''\nmodel=''\napi_key=''\n\n[policy]\napproval='{}'\nsandbox='workspace-write'\n\n[sandbox]\ncpu_limit=1\nmem_limit_mb=64\nnet='off'\n\n[git]\nconventional=true\nmax_staged_files=10\n",
        approval
    );
    fs::write(dir.join("devit.toml"), cfg).unwrap();
}

#[test]
fn run_on_request_without_yes_fails_early() {
    // create a temp dir manually
    let mut d = std::env::temp_dir();
    let uniq = format!("devit-test-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis());
    d.push(uniq);
    fs::create_dir_all(&d).unwrap();
    write_cfg(&d, "on-request");

    // Locate the built binary for this package
    let bin = env!("CARGO_BIN_EXE_devit");
    let out = Command::new(bin)
        .current_dir(&d)
        .arg("run")
        .arg("--goal").arg("demo")
        .output()
        .expect("failed to execute binary");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nécessite --yes"));
}
