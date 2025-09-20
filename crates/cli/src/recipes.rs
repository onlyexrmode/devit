use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Deserialize, Debug)]
struct RecipeFile {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    steps: Vec<RecipeStep>,
}

#[derive(Deserialize, Debug)]
struct RecipeStep {
    #[serde(rename = "kind")]
    kind: RecipeKind,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    run: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum RecipeKind {
    Shell,
    Git,
    Devit,
}

#[derive(serde::Serialize)]
pub struct RecipeSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(serde::Serialize)]
pub struct RecipeRunReport {
    pub id: String,
    pub dry_run: bool,
    pub steps_total: usize,
}

pub struct RecipeRunError {
    pub payload: serde_json::Value,
    pub exit_code: i32,
}

const DEFAULT_RECIPES_DIR: &str = ".devit/recipes";
const ENV_RECIPES_DIR: &str = "DEVIT_RECIPES_DIR";

fn recipes_dir() -> PathBuf {
    if let Ok(custom) = env::var(ENV_RECIPES_DIR) {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    PathBuf::from(DEFAULT_RECIPES_DIR)
}

fn load_recipe_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "yml" | "yaml") {
                        out.push(path);
                    }
                }
            }
        }
    }
    out.sort();
    out
}

fn load_recipe(path: &Path) -> Result<RecipeFile> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("read recipe {}", path.display()))?;
    let recipe: RecipeFile = serde_yaml::from_str(&contents)
        .with_context(|| format!("parse recipe {}", path.display()))?;
    if recipe.id.trim().is_empty() {
        return Err(anyhow!("recipe id is empty"));
    }
    if recipe.name.trim().is_empty() {
        return Err(anyhow!("recipe name is empty"));
    }
    Ok(recipe)
}

pub fn list_recipes() -> Result<Vec<RecipeSummary>> {
    let dir = recipes_dir();
    let mut recipes = Vec::new();
    for path in load_recipe_files(&dir) {
        match load_recipe(&path) {
            Ok(file) => recipes.push(RecipeSummary {
                id: file.id,
                name: file.name,
                description: file.description,
            }),
            Err(e) => {
                eprintln!("warn: skip recipe {} ({})", path.display(), e);
            }
        }
    }
    Ok(recipes)
}

pub fn run_recipe(id: &str, dry_run: bool) -> Result<RecipeRunReport, RecipeRunError> {
    let dir = recipes_dir();
    let mut selected: Option<RecipeFile> = None;
    for path in load_recipe_files(&dir) {
        match load_recipe(&path) {
            Ok(file) => {
                if file.id == id {
                    selected = Some(file);
                    break;
                }
            }
            Err(e) => {
                eprintln!("warn: skip recipe {} ({})", path.display(), e);
            }
        }
    }

    let recipe = match selected {
        Some(r) => r,
        None => {
            return Err(RecipeRunError {
                payload: json!({
                    "recipe_require_failed": true,
                    "reason": "not_found",
                    "id": id,
                }),
                exit_code: 2,
            });
        }
    };

    for (idx, step) in recipe.steps.iter().enumerate() {
        let label = step.name.as_deref().unwrap_or_else(|| step.kind.as_str());
        if dry_run {
            eprintln!(
                "[dry-run] step {} ({}): {}",
                idx + 1,
                step.kind.as_str(),
                label
            );
            continue;
        }

        eprintln!(
            "[recipe] step {} ({}): {}",
            idx + 1,
            step.kind.as_str(),
            label
        );
        if let Err(e) = execute_step(step) {
            return Err(RecipeRunError {
                payload: json!({
                    "recipe_require_failed": true,
                    "reason": e,
                    "step": idx + 1,
                    "id": id,
                }),
                exit_code: 1,
            });
        }
    }

    Ok(RecipeRunReport {
        id: recipe.id,
        dry_run,
        steps_total: recipe.steps.len(),
    })
}

fn execute_step(step: &RecipeStep) -> Result<(), String> {
    match step.kind {
        RecipeKind::Shell => {
            let command = step
                .run
                .as_ref()
                .ok_or_else(|| "shell step missing 'run'".to_string())?;
            Command::new("bash")
                .arg("-lc")
                .arg(command)
                .status()
                .map_err(|e| e.to_string())
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("shell exit code {}", status.code().unwrap_or(-1)))
                    }
                })
        }
        RecipeKind::Git => {
            let args = step
                .args
                .as_ref()
                .ok_or_else(|| "git step missing 'args'".to_string())?;
            if args.is_empty() {
                return Err("git step args empty".into());
            }
            Command::new("git")
                .args(args)
                .status()
                .map_err(|e| e.to_string())
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("git exit code {}", status.code().unwrap_or(-1)))
                    }
                })
        }
        RecipeKind::Devit => {
            let args = step
                .args
                .as_ref()
                .ok_or_else(|| "devit step missing 'args'".to_string())?;
            if args.is_empty() {
                return Err("devit step args empty".into());
            }
            Command::new(env::current_exe().unwrap_or_else(|_| PathBuf::from("devit")))
                .args(args)
                .status()
                .map_err(|e| e.to_string())
                .and_then(|status| {
                    if status.success() {
                        Ok(())
                    } else {
                        Err(format!("devit exit code {}", status.code().unwrap_or(-1)))
                    }
                })
        }
    }
}

impl RecipeKind {
    fn as_str(&self) -> &'static str {
        match self {
            RecipeKind::Shell => "shell",
            RecipeKind::Git => "git",
            RecipeKind::Devit => "devit",
        }
    }
}
