#![cfg(feature = "experimental")]
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

#[test]
fn watchdog_exits_after_deadline() {
    let Some(exe) = option_env!("CARGO_BIN_EXE_devit-mcpd") else {
        eprintln!("skipping: devit-mcpd bin not built for tests");
        return;
    };
    let mut child = Command::new(exe)
        .arg("--yes")
        .arg("--max-runtime-secs")
        .arg("1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn devit-mcpd");

    // Feed periodic pings so the loop iterates and checks the watchdog.
    let mut sin = child.stdin.take().expect("stdin");
    let feeder = thread::spawn(move || {
        for _ in 0..20 {
            let _ = writeln!(sin, "{{\"type\":\"ping\"}}");
            thread::sleep(Duration::from_millis(100));
        }
    });

    // Wait up to 3s for process to exit due to watchdog
    let status = (|| {
        for _ in 0..30 {
            if let Some(s) = child.try_wait().expect("try_wait") {
                return s;
            }
            thread::sleep(Duration::from_millis(100));
        }
        child.kill().ok();
        child.wait().expect("wait after kill")
    })();

    let stderr = {
        let mut s = String::new();
        if let Some(mut e) = child.stderr {
            use std::io::Read;
            let _ = e.read_to_string(&mut s);
        }
        s
    };

    assert!(!status.success(), "expected non-zero exit");
    assert_eq!(status.code().unwrap_or_default(), 2, "exit code must be 2");
    assert!(
        stderr.contains("max runtime exceeded"),
        "stderr must mention watchdog: {}",
        stderr
    );
    let _ = feeder.join();
}
