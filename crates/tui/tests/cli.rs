use assert_cmd::cargo::cargo_bin;
use serde_json::Value;
use std::fs::File;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

fn with_timeout<F, R>(duration: Duration, f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = panic::catch_unwind(AssertUnwindSafe(f));
        let _ = tx.send(result);
    });

    match rx.recv_timeout(duration) {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => panic::resume_unwind(err),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("test timed out after {:?}", duration)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("test worker disconnected without signalling completion")
        }
    }
}

#[test]
fn missing_journal_exits_1() {
    with_timeout(Duration::from_secs(5), || {
        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        let assert = cmd
            .arg("--journal-path")
            .arg("/no/such/file.jsonl")
            .assert();
        let output = assert.get_output();
        assert!(output.status.code().unwrap_or(0) != 0);
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(err.contains("journal_not_found"));
    });
}

#[test]
fn start_with_journal_and_quit() {
    with_timeout(Duration::from_secs(5), || {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("journal.jsonl");
        let mut f = File::create(&p).unwrap();
        for i in 0..10 {
            writeln!(f, "{{\"type\":\"test\",\"n\":{}}}", i).unwrap();
        }
        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--journal-path").arg(&p);
        cmd.assert().success();
    });
}

#[test]
fn open_diff_headless_from_file() {
    with_timeout(Duration::from_secs(5), || {
        let dir = tempfile::tempdir().unwrap();
        let diff_path = dir.path().join("sample.diff");
        let mut f = File::create(&diff_path).unwrap();
        writeln!(
            f,
            "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-old\n+new"
        )
        .unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open").arg(&diff_path);
        cmd.assert().success();
    });
}

#[test]
fn open_diff_headless_from_stdin() {
    with_timeout(Duration::from_secs(5), || {
        let diff = "diff --git a/foo b/foo\n--- a/foo\n+++ b/foo\n@@ -1 +1 @@\n-old\n+new\n";
        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open").arg("-");
        cmd.write_stdin(diff);
        cmd.assert().success();
    });
}

#[test]
fn open_diff_missing_file_reports_error() {
    with_timeout(Duration::from_secs(5), || {
        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open").arg("/no/such/diff.patch");
        let assert = cmd.assert().failure();
        let output = assert.get_output();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("diff_load_failed"));
        assert!(stderr.contains("not_found"));
        assert_eq!(output.status.code(), Some(2));
    });
}

#[test]
fn headless_open_log_prints_last_event() {
    with_timeout(Duration::from_secs(5), || {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("journal.jsonl");
        let mut f = File::create(&journal).unwrap();
        writeln!(f, "{{\"type\":\"test\",\"n\":1}}").unwrap();
        writeln!(f, "{{\"type\":\"test\",\"n\":2}}").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open-log").arg(&journal);
        let assert = cmd.assert().success();
        let output = assert.get_output();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"n\": 2"), "stdout: {stdout}");
    });
}

#[test]
fn headless_open_log_seek_last_limits_window() {
    with_timeout(Duration::from_secs(5), || {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("journal.jsonl");
        let mut f = File::create(&journal).unwrap();
        writeln!(f, "{{\"type\":\"test\",\"n\":1}}").unwrap();
        writeln!(f, "{{\"type\":\"test\",\"n\":2}}").unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open-log")
            .arg(&journal)
            .arg("--seek-last")
            .arg("1");
        let assert = cmd.assert().success();
        let output = assert.get_output();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("\"n\": 2"), "stdout: {stdout}");
        assert!(!stdout.contains("\"n\": 1"), "stdout: {stdout}");
    });
}

#[test]
fn headless_open_log_truncates_large_event() {
    with_timeout(Duration::from_secs(5), || {
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join("journal.jsonl");
        let mut f = File::create(&journal).unwrap();
        let payload = format!("{{\"type\":\"blob\",\"data\":\"{}\"}}", "a".repeat(5000));
        writeln!(f, "{}", payload).unwrap();

        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.timeout(Duration::from_secs(5));
        cmd.arg("--open-log").arg(&journal);
        let assert = cmd.assert().success();
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
        assert!(stdout.contains("... (truncated)"), "stdout: {stdout}");
    });
}

#[test]
fn list_recipes_headless_outputs_json() {
    with_timeout(Duration::from_secs(5), || {
        let tmp = tempfile::tempdir().unwrap();
        let recipes_dir = tmp.path().join(".devit/recipes");
        std::fs::create_dir_all(&recipes_dir).unwrap();
        let recipe_path = recipes_dir.join("demo.yaml");
        let mut file = File::create(&recipe_path).unwrap();
        writeln!(file, "id: demo").unwrap();
        writeln!(file, "name: Demo Recipe").unwrap();

        let devit_bin = cargo_bin("devit");
        let config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../devit.toml");
        let mut cmd = assert_cmd::Command::cargo_bin("devit-tui").unwrap();
        cmd.env("DEVIT_TUI_DEVIT_BIN", devit_bin);
        cmd.env("DEVIT_TUI_HEADLESS", "1");
        cmd.env("DEVIT_CONFIG", config_path);
        cmd.current_dir(tmp.path());
        let assert = cmd.arg("--list-recipes").assert().success();
        let output = assert.get_output();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let value: Value = serde_json::from_str(stdout.trim()).unwrap();
        assert!(value.get("recipes").is_some());
        let recipes = value.get("recipes").unwrap().as_array().unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].get("id").unwrap().as_str(), Some("demo"));
    });
}
