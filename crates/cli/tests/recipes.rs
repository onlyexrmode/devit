use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;
use std::fs;

fn setup_recipes() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let recipe_dir = dir.path().join("recipes");
    fs::create_dir_all(&recipe_dir).unwrap();
    fs::write(
        dir.path().join("devit.toml"),
        "[backend]\nkind='openai_like'\nbase_url=''\nmodel=''\napi_key=''\n\n[policy]\napproval='never'\nsandbox='workspace-write'\n\n[sandbox]\ncpu_limit=1\nmem_limit_mb=64\nnet='off'\n\n[git]\nconventional=false\nmax_staged_files=10\n",
    )
    .unwrap();

    let add_ci = r#"
        id: add-ci
        name: "Add CI"
        description: "Add a CI workflow"
        steps:
          - kind: shell
            name: "echo"
            run: "echo add-ci"
    "#;
    let rust_upgrade = r#"
        id: rust-upgrade-1.81
        name: "Rust upgrade 1.81"
        steps:
          - kind: shell
            run: "echo upgrade"
    "#;
    let migrate = r#"
        id: migrate-jest-vitest
        name: "Migrate Jest to Vitest"
        steps:
          - kind: shell
            run: "echo migrate"
    "#;

    fs::write(recipe_dir.join("add-ci.yaml"), add_ci).unwrap();
    fs::write(recipe_dir.join("rust-upgrade-1.81.yaml"), rust_upgrade).unwrap();
    fs::write(recipe_dir.join("migrate-jest-vitest.yaml"), migrate).unwrap();

    (dir, recipe_dir)
}

#[test]
fn recipe_list_outputs_json() {
    let (tmp, recipe_dir) = setup_recipes();
    let mut cmd = Command::cargo_bin("devit").unwrap();
    cmd.current_dir(tmp.path())
        .env("DEVIT_RECIPES_DIR", recipe_dir)
        .arg("recipe")
        .arg("list");
    let output = cmd.assert().success().get_output().stdout.clone();
    let value: Value = serde_json::from_slice(&output).unwrap();
    let recipes = value
        .get("recipes")
        .and_then(|v| v.as_array())
        .expect("recipes array");
    let ids: Vec<&str> = recipes
        .iter()
        .filter_map(|r| r.get("id").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains(&"add-ci"));
    assert!(ids.contains(&"rust-upgrade-1.81"));
    assert!(ids.contains(&"migrate-jest-vitest"));
}

#[test]
fn recipe_run_dry_run_works() {
    let (tmp, recipe_dir) = setup_recipes();
    let mut cmd = Command::cargo_bin("devit").unwrap();
    cmd.current_dir(tmp.path())
        .env("DEVIT_RECIPES_DIR", recipe_dir)
        .arg("recipe")
        .arg("run")
        .arg("add-ci")
        .arg("--dry-run");
    cmd.assert().success().stdout(contains("\"ok\":true"));
}
