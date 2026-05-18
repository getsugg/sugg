#![allow(dead_code)]

use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn sugg_bin() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    if path.file_name().map(|n| n == "deps").unwrap_or(false) {
        path.pop();
    }
    if cfg!(windows) {
        path.push("sugg.exe");
    } else {
        path.push("sugg");
    }
    path
}

pub fn sugg_engine_bin() -> PathBuf {
    let mut path = env::current_exe().unwrap();
    path.pop();
    if path.file_name().map(|n| n == "deps").unwrap_or(false) {
        path.pop();
    }
    if cfg!(windows) {
        path.push("sugg-engine.exe");
    } else {
        path.push("sugg-engine");
    }
    path
}

pub fn reload(cache_dir: &PathBuf, completions_dir: &Path) {
    let status = Command::new(sugg_bin())
        .arg("reload")
        .arg("--cache-dir")
        .arg(cache_dir)
        .arg("--completions-dir")
        .arg(completions_dir)
        .status()
        .expect("failed to run reload");
    assert!(status.success(), "reload failed");
}
#[allow(dead_code)]
pub fn reload_with_lang(cache_dir: &PathBuf, completions_dir: &Path, lang: &str) {
    let status = Command::new(sugg_bin())
        .arg("reload")
        .arg("--cache-dir")
        .arg(cache_dir)
        .arg("--completions-dir")
        .arg(completions_dir)
        .arg("--lang")
        .arg(lang)
        .status()
        .expect("failed to run reload");
    assert!(status.success(), "reload failed");
}

pub fn complete(input: &str, project_dir: &Path, cache_dir: &PathBuf) -> Vec<Value> {
    let output = Command::new(sugg_bin())
        .arg("complete")
        .arg("nushell")
        .current_dir(project_dir)
        .arg("--cache-dir")
        .arg(cache_dir)
        .arg("--")
        .arg(input)
        .output()
        .expect("failed to run complete");

    if !output.stderr.is_empty() {
        eprintln!(
            ">>> 子进程 Stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert!(output.status.success(), "sugg command failed");
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout).expect("failed to parse JSON")
}

pub fn has_item(items: &[Value], value: &str) -> bool {
    let expected_with_space = format!("{} ", value);
    items.iter().any(|item| {
        item["value"] == expected_with_space || item["value"] == value
    })
}

#[allow(dead_code)]
pub fn get_style_for_value(items: &[Value], value: &str) -> Option<Value> {
    let expected = format!("{} ", value);
    items
        .iter()
        .find(|item| item["value"] == expected)
        .and_then(|item| item.get("style").cloned())
}

pub fn get_fixture_dir(sub_path: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.push(sub_path);
    path
}

#[allow(dead_code)]
pub fn normalize_items(items: &[Value], project_dir: &Path) -> Vec<Value> {
    let mut sorted: Vec<_> = items.to_vec();
    sorted.sort_by(|a, b| {
        a["value"]
            .as_str()
            .unwrap_or("")
            .cmp(b["value"].as_str().unwrap_or(""))
    });
    let json_str = serde_json::to_string(&sorted).unwrap();
    let proj_str = project_dir.to_string_lossy();
    let normalized = json_str.replace(proj_str.as_ref(), "<PROJECT_ROOT>");
    serde_json::from_str(&normalized).unwrap()
}

#[allow(dead_code)]
pub fn snapshot_complete(
    snapshot_name: &str,
    input: &str,
    project_dir: &Path,
    cache_dir: &PathBuf,
) {
    let items = complete(input, project_dir, cache_dir);
    let normalized = normalize_items(&items, project_dir);
    insta::assert_json_snapshot!(snapshot_name, normalized);
}

pub struct SharedSandbox {
    root_path: PathBuf,
    pub cache_dir: PathBuf,
    pub project_dirs: HashMap<String, PathBuf>,
}

impl Drop for SharedSandbox {
    fn drop(&mut self) {
        if let Err(e) = remove_dir_all(&self.root_path) {
            eprintln!("WARNING: sandbox {:?} not deleted: {}", self.root_path, e);
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir_recursive(&s, &d);
        } else {
            fs::copy(&s, &d).unwrap_or_else(|e| panic!("无法复制 {:?}: {}", s, e));
        }
    }
}

fn remove_dir_all(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            remove_dir_all(&p)?;
        } else {
            // 去掉只读属性再删
            let mut perms = fs::metadata(&p)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(&p, perms);
            fs::remove_file(&p)?;
        }
    }
    fs::remove_dir(path)
}

static SHARED_SANDBOX: OnceLock<SharedSandbox> = OnceLock::new();

pub fn get_sandbox() -> &'static SharedSandbox {
    SHARED_SANDBOX.get_or_init(|| {
        // 使用固定路径，每次初始化时先清理，避免 static Drop 不被调用导致无限堆积
        let temp_root = env::temp_dir().join("sugg_test_sandbox");

        // 清理上次遗留
        if temp_root.exists() {
            let _ = remove_dir_all(&temp_root);
        }
        fs::create_dir_all(&temp_root).unwrap();

        let cache_dir = temp_root.join(".cache");
        fs::create_dir_all(&cache_dir).unwrap();

        let completions_dir = temp_root.join("completions");
        let fixture_completions_dir = get_fixture_dir("completions");
        copy_dir_recursive(&fixture_completions_dir, &completions_dir);

        reload(&cache_dir, &completions_dir);

        let mut project_dirs = HashMap::new();
        for name in &["minimal", "pnpm", "bun"] {
            project_dirs.insert(
                name.to_string(),
                get_fixture_dir(&format!("projects/{}", name)),
            );
        }

        SharedSandbox {
            root_path: temp_root,
            cache_dir,
            project_dirs,
        }
    })
}
