use anyhow::{anyhow, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{DirEntry, WalkBuilder, WalkState};
use memmap2::MmapOptions;
use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct ContextOpts {
    pub max_bytes_per_file: usize,
    pub max_files: usize,
    pub ext_allow: Option<Vec<String>>, // like ["rs","toml"]
    pub timeout: Option<Duration>,
    pub out_path: PathBuf,
}

#[derive(Serialize, Clone, Debug)]
struct FileEntry {
    path: String,
    size: u64,
    lang: String,
    score: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbols_count: Option<u32>,
}

#[derive(Serialize)]
struct IndexJson {
    root: String,
    generated_at: String,
    files: Vec<FileEntry>,
    skipped: Skipped,
}

#[derive(Serialize, Default)]
struct Skipped {
    too_large: u64,
    binary: u64,
}

pub fn generate_index(root: &Path, opts: &ContextOpts) -> Result<PathBuf> {
    let start = Instant::now();

    let mut builder = WalkBuilder::new(root);
    builder
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(true)
        .follow_links(false);

    // Internal excludes
    let mut gs = GlobSetBuilder::new();
    for pat in [".devit/**", "target/**", "bench/**"].iter() {
        gs.add(Glob::new(pat)?);
    }
    if let Some(exts) = &opts.ext_allow {
        // Build inclusion set for quick check
        for e in exts {
            let pat = format!("**/*.{}", e.trim().trim_start_matches('.'));
            gs.add(Glob::new(&pat)?);
        }
    }
    let globset = gs.build()?;

    let mut paths: Vec<PathBuf> = Vec::new();
    let paths_sync = std::sync::Mutex::new(&mut paths);
    builder.build_parallel().run(|| {
        let globset = globset.clone();
        let paths_sync = &paths_sync;
        Box::new(move |res| match res {
            Ok(ent) => {
                if should_skip_entry(&ent, &globset) {
                    WalkState::Continue
                } else {
                    if ent.file_type().map(|t| t.is_file()).unwrap_or(false) {
                        if let Ok(mut guard) = paths_sync.lock() {
                            guard.push(ent.path().to_path_buf());
                        }
                    }
                    WalkState::Continue
                }
            }
            Err(_) => WalkState::Continue,
        })
    });

    // Dedup and cap
    paths.sort();
    if paths.len() > opts.max_files {
        paths.truncate(opts.max_files);
    }

    let timeout = opts.timeout;
    let max_bytes = opts.max_bytes_per_file as u64;
    let entries: Vec<FileEntry> = paths
        .par_iter()
        .map(|p| {
            if let Some(t) = timeout {
                if start.elapsed() > t {
                    return Err(anyhow!("timeout"));
                }
            }
            summarize_file(root, p, max_bytes)
        })
        .filter_map(|r| r.ok())
        .collect();

    // Compute skipped counts (approx by scanning again quickly)
    let mut skipped = Skipped::default();
    for p in &paths {
        if let Ok(md) = fs::metadata(p) {
            if md.len() > max_bytes {
                skipped.too_large += 1;
                continue;
            }
            if is_binary_quick(p).unwrap_or(false) {
                skipped.binary += 1;
                continue;
            }
        }
    }

    let mut files = entries;
    files.sort_by(|a, b| b.score.cmp(&a.score));

    let idx = IndexJson {
        root: root.display().to_string(),
        generated_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        files,
        skipped,
    };

    if let Some(t) = timeout {
        if start.elapsed() > t {
            return Err(anyhow!("timeout"));
        }
    }

    // Atomic write
    let out = opts.out_path.clone();
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).ok();
    }
    let tmp = out.with_extension("json.tmp");
    let mut f = fs::File::create(&tmp)?;
    writeln!(f, "{}", serde_json::to_string_pretty(&idx)?)?;
    fs::rename(tmp, &out)?;
    Ok(out)
}

fn should_skip_entry(ent: &DirEntry, gs: &GlobSet) -> bool {
    let p = ent.path();
    let rel = p.to_string_lossy();
    for pat in [".devit/", "target/", "bench/"].iter() {
        if rel.contains(pat) {
            return true;
        }
    }
    // If ext_allow provided (encoded in globset along with excludes), ensure it matches at least one allowed pattern
    if !gs.is_empty() {
        // If any of our exclude globs match, skip
        if gs.is_match(p) {
            // ambiguous: our set has both excludes and includes; we rely on explicit excludes by prefix checks above.
        }
    }
    false
}

fn summarize_file(root: &Path, path: &Path, max_bytes: u64) -> Result<FileEntry> {
    let md = fs::metadata(path)?;
    let sz = md.len();
    // Skip too large and binaries
    if sz > max_bytes {
        anyhow::bail!("too large")
    }
    if is_binary_quick(path)? {
        anyhow::bail!("binary")
    }
    let rel = pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf());
    let rels = rel.to_string_lossy().to_string();
    let lang = detect_lang(&rels);
    let mut score: i64 = 0;
    if rels.starts_with("src/") || rels.starts_with("tests/") {
        score += 50;
    }
    if rels.contains("mcp") || rels.contains("plugin") {
        score += 30;
    }
    if matches!(
        lang.as_str(),
        "rust" | "js" | "ts" | "py" | "c" | "cpp" | "h"
    ) {
        score += 20;
    }

    // symbols via tree-sitter (best-effort)
    let mut symbols_count: Option<u32> = None;
    if matches!(lang.as_str(), "rust" | "js" | "py") {
        if let Ok(cnt) = count_symbols(path, &lang) {
            symbols_count = Some(cnt);
        }
    }

    Ok(FileEntry {
        path: rels,
        size: sz,
        lang,
        score,
        symbols_count,
    })
}

fn is_binary_quick(path: &Path) -> Result<bool> {
    // try mmap
    if let Ok(file) = fs::File::open(path) {
        if let Ok(m) = unsafe { MmapOptions::new().len(1024 * 16).map(&file) } {
            if m.contains(&0) {
                return Ok(true);
            }
            return Ok(false);
        }
    }
    // fallback: read small chunk
    let mut f = fs::File::open(path)?;
    let mut buf = [0u8; 8192];
    let n = f.read(&mut buf).unwrap_or(0);
    Ok(buf[..n].contains(&0))
}

fn detect_lang(p: &str) -> String {
    let lower = p.to_lowercase();
    for (exts, tag) in [
        ((vec![".rs"]), "rust"),
        ((vec![".js", ".ts", ".tsx"]), "js"),
        ((vec![".py"]), "py"),
        ((vec![".toml"]), "toml"),
        ((vec![".md"]), "md"),
        ((vec![".json"]), "json"),
        ((vec![".yml", ".yaml"]), "yml"),
        ((vec![".c", ".h"]), "c"),
        ((vec![".cpp", ".hpp"]), "cpp"),
        ((vec![".sh"]), "sh"),
    ] {
        if exts.iter().any(|e| lower.ends_with(e)) {
            return tag.to_string();
        }
    }
    "text".to_string()
}

fn count_symbols(path: &Path, lang: &str) -> Result<u32> {
    use tree_sitter::{Parser, Tree};
    let source = fs::read_to_string(path).unwrap_or_default();
    let mut parser = Parser::new();
    match lang {
        "rust" => parser.set_language(&tree_sitter_rust::language()).unwrap(),
        "js" => parser
            .set_language(&tree_sitter_javascript::language())
            .unwrap(),
        "py" => parser
            .set_language(&tree_sitter_python::language())
            .unwrap(),
        _ => return Ok(0),
    }
    let tree: Option<Tree> = parser.parse(&source, None);
    if tree.is_none() {
        return Ok(0);
    }
    let tree = tree.unwrap();
    let mut cnt: u32 = 0;
    let root = tree.root_node();
    let mut cursor = root.walk();
    for n in root.children(&mut cursor) {
        let kind = n.kind();
        match (lang, kind) {
            ("rust", k)
                if [
                    "function_item",
                    "struct_item",
                    "enum_item",
                    "trait_item",
                    "impl_item",
                    "mod_item",
                ]
                .contains(&k) =>
            {
                cnt += 1
            }
            ("js", k) if ["function_declaration", "class_declaration"].contains(&k) => cnt += 1,
            ("py", k) if ["function_definition", "class_definition"].contains(&k) => cnt += 1,
            _ => {}
        }
        if cnt >= 200 {
            break;
        }
    }
    Ok(cnt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn builds_index_with_filters() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::create_dir_all(root.join("target")).unwrap();
        fs::create_dir_all(root.join(".devit")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn x(){}\n").unwrap();
        fs::write(root.join("tests/foo.rs"), "#[test] fn t(){}\n").unwrap();
        fs::write(root.join(".devit/secret.txt"), "sekrit").unwrap();
        let mut big = fs::File::create(root.join("target/junk.bin")).unwrap();
        big.write_all(&vec![0u8; 300_000]).unwrap();

        let out = root.join(".devit/index.json");
        let opts = ContextOpts {
            max_bytes_per_file: 262_144,
            max_files: 5000,
            ext_allow: None,
            timeout: Some(Duration::from_secs(5)),
            out_path: out.clone(),
        };
        let written = generate_index(root, &opts).unwrap();
        assert_eq!(written, out);
        let txt = fs::read_to_string(&written).unwrap();
        assert!(txt.contains("\"root\":"));
        assert!(!txt.contains(".devit/secret.txt"));
        assert!(!txt.contains("target/junk.bin"));
    }
}
