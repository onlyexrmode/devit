use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub fn sarif_latest() -> Result<PathBuf> {
    let p = Path::new(".devit/reports/sarif.json");
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let skeleton = serde_json::json!({
            "version": "2.1.0",
            "runs": []
        });
        fs::write(p, serde_json::to_vec(&skeleton)?)?;
    }
    Ok(p.to_path_buf())
}

pub fn junit_latest() -> Result<PathBuf> {
    let p = Path::new(".devit/reports/junit.xml");
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let content = r#"<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<testsuites><testsuite name=\"empty\" tests=\"0\" failures=\"0\" time=\"0\"/></testsuites>
"#;
        fs::write(p, content)?;
    }
    Ok(p.to_path_buf())
}

