use std::fs;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn write_cfg(dir: &std::path::Path, approval: &str) {
    let cfg = format!(
        "[backend]\nkind='openai_like'\nbase_url=''\nmodel=''\napi_key=''\n\n[policy]\napproval='{}'\nsandbox='workspace-write'\n\n[sandbox]\ncpu_limit=1\nmem_limit_mb=64\nnet='off'\n\n[git]\nconventional=true\nmax_staged_files=10\nuse_notes=false\n",
        approval
    );
    fs::write(dir.join("devit.toml"), cfg).unwrap();
}

fn tmpdir() -> std::path::PathBuf {
    let mut d = std::env::temp_dir();
    let uniq = format!(
        "devit-test-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    d.push(uniq);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn shell_exec_json_outputs() {
    let d = tmpdir();
    write_cfg(&d, "never");
    let bin = env!("CARGO_BIN_EXE_devit");

    let input = serde_json::json!({
        "name": "shell_exec",
        "args": {"cmd": "echo hello | tr a-z A-Z"},
        "yes": true
    })
    .to_string();

    let out = Command::new(bin)
        .current_dir(&d)
        .arg("tool")
        .arg("call")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(input.as_bytes())?;
            child.wait_with_output()
        })
        .expect("failed to run devit");

    assert!(out.status.success());
    let resp: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    let res = resp.get("result").cloned().unwrap_or(serde_json::json!({}));
    assert_eq!(res.get("exit_code").and_then(|v| v.as_i64()).unwrap_or(-1), 0);
    let out_txt = res.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string();
    assert!(out_txt.contains("HELLO"));
}

#[test]
fn fs_patch_apply_check_only_succeeds() {
    let d = tmpdir();
    write_cfg(&d, "never");

    // init repo
    assert!(Command::new("git").current_dir(&d).args(["init"]).status().unwrap().success());
    fs::write(d.join("f.txt"), "one\n").unwrap();
    assert!(Command::new("git").current_dir(&d).args(["add", "."]).status().unwrap().success());
    assert!(Command::new("git").current_dir(&d).args(["commit", "-m", "init"]).status().unwrap().success());

    // prepare a minimal unified diff (add a line)
    let diff_txt = "--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,2 @@\n one\n+two\n".to_string();

    let req = serde_json::json!({
        "name": "fs_patch_apply",
        "args": {"patch": diff_txt, "check_only": true},
        "yes": true
    })
    .to_string();

    let bin = env!("CARGO_BIN_EXE_devit");
    let out = Command::new(bin)
        .current_dir(&d)
        .arg("tool")
        .arg("call")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(req.as_bytes())?;
            child.wait_with_output()
        })
        .expect("failed to run devit");

    assert!(out.status.success());
    let resp: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    let res = resp.get("result").cloned().unwrap_or(serde_json::json!({}));
    assert!(res.get("checked").and_then(|v| v.as_bool()).unwrap_or(false));
}
