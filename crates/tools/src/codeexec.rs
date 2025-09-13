use anyhow::{anyhow, Result};
use std::process::{Command, Stdio};

pub enum Stack
{
    Cargo,
    Npm,
    CMake,
    Unknown,
}

pub fn detect_stack() -> Stack
{
    if std::path::Path::new("Cargo.toml").exists() { return Stack::Cargo; }
    if std::path::Path::new("package.json").exists() { return Stack::Npm; }
    if std::path::Path::new("CMakeLists.txt").exists() { return Stack::CMake; }
    Stack::Unknown
}

pub fn run_tests() -> Result<i32>
{
    match detect_stack() {
        Stack::Cargo => {
            let status = Command::new("cargo")
                .args(["test", "--all", "--quiet"])
                .status()?;
            Ok(status.code().unwrap_or(-1))
        }
        Stack::Npm => {
            let status = Command::new("npm")
                .args(["test", "--silent"])
                .status()?;
            Ok(status.code().unwrap_or(-1))
        }
        Stack::CMake => {
            let status = Command::new("ctest")
                .args(["--output-on-failure"])
                .status();
            match status {
                Ok(s) => Ok(s.code().unwrap_or(-1)),
                Err(_) => Err(anyhow!("Aucun runner de tests CMake/ctest détecté")),
            }
        }
        Stack::Unknown => Err(anyhow!("Stack inconnue: impossible d'exécuter les tests")),
    }
}

pub fn run_tests_with_output() -> Result<(i32, String)>
{
    match detect_stack() {
        Stack::Cargo => {
            let out = Command::new("cargo")
                .args(["test", "--all"])
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .output()?;
            let txt = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr).to_string();
            Ok((out.status.code().unwrap_or(-1), txt))
        }
        Stack::Npm => {
            let out = Command::new("npm")
                .args(["test"])
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .output()?;
            let txt = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr).to_string();
            Ok((out.status.code().unwrap_or(-1), txt))
        }
        Stack::CMake => {
            let out = Command::new("ctest")
                .args(["--output-on-failure"])
                .stdout(Stdio::piped()).stderr(Stdio::piped())
                .output()?;
            let txt = String::from_utf8_lossy(&out.stdout).to_string()
                + &String::from_utf8_lossy(&out.stderr).to_string();
            Ok((out.status.code().unwrap_or(-1), txt))
        }
        Stack::Unknown => Err(anyhow!("Stack inconnue")),
    }
}
