//! Sugg Engine - 引擎构建端
//!
//! 包含所有重型依赖（oxc, rolldown, icu）：
//! - `ast`: AST 提取与分析
//! - `bundler`: 虚拟模块插件与打包
//! - `icu_utils`: ICU 语言回退链生成

pub use sugg_ast as ast;
pub mod build;
pub mod bundler;
pub mod i18n;
pub mod icu_utils;
pub mod install;
pub mod locale;

pub use icu_utils::get_fallback_chain;
pub use locale::detect_locale;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::bundler::codegen::{generate_env_code, generate_i18n_modules, generate_per_file_entry};
use crate::bundler::{VIRTUAL_DYNAMIC_ENTRY, bundle_virtual};
use anyhow::Context;
use sugg_core::cache::CommandNode;
use sugg_core::log_warn;

/// 单文件缓存，用于增量热更时跳过未变化文件的 AST 提取、rolldown 打包和 QuickJS 评估。
pub struct CachedFile {
    pub content_hash: u64,
    pub stem: String,
    pub mod_source: String,
    pub dyn_source: String,
    pub func_ids: Vec<String>,
    /// 本地依赖文件的绝对路径 → 当前依赖文件的 content_hash
    pub deps: HashMap<PathBuf, u64>,
    /// 缓存的单文件静态 rolldown 输出
    pub static_bundle: Option<String>,
    /// 缓存的 QuickJS __parseConfig 结果
    pub command_node: Option<CommandNode>,
    /// 缓存的单文件动态 rolldown 输出
    pub dyn_bundle: Option<String>,
    /// 缓存的 QuickJS bytecode
    pub bytecode: Option<Vec<u8>>,
}

/// 检查缓存的依赖路径 content_hash 是否与磁盘一致。
/// 所有 dep 文件必须存在且 hash 匹配才算 unchanged。
fn deps_unchanged(deps: &HashMap<PathBuf, u64>) -> bool {
    for (dep_path, cached_hash) in deps {
        match std::fs::read_to_string(dep_path) {
            Ok(s) => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                s.hash(&mut h);
                if h.finish() != *cached_hash {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// 扫描 `dir_path` 下的补全脚本，用指定 `lang` 打包，返回 `(per_file_static, per_file_dynamic)`。
/// 每个条目对应一个文件的打包结果，未变化文件从 `cache` 直接返回缓存产物。
pub async fn build_bundles(
    dir_path: &Path,
    lang: &str,
    cache: &mut HashMap<PathBuf, CachedFile>,
) -> anyhow::Result<(
    Vec<(String, String)>,              // per-file static: (stem, bundled_code)
    Vec<(String, String, Vec<String>)>, // per-file dynamic: (stem, bundled_code, func_ids)
)> {
    let load_json = |p: &Path| -> HashMap<String, String> {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };
    let mut translations_by_ns: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (ns, i18n_dir) in scan_i18n_dirs(dir_path) {
        let fallbacks = get_fallback_chain(lang);

        let mut map = HashMap::new();
        for (_, fb_path) in get_matching_i18n_files(&i18n_dir, &fallbacks) {
            map.extend(load_json(&fb_path));
        }
        translations_by_ns.insert(ns, map);
    }

    let i18n_hash = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut ns_keys: Vec<&String> = translations_by_ns.keys().collect();
        ns_keys.sort();
        for ns in &ns_keys {
            ns.hash(&mut h);
            if let Some(trans) = translations_by_ns.get(*ns) {
                let mut trans_keys: Vec<&String> = trans.keys().collect();
                trans_keys.sort();
                for key in &trans_keys {
                    key.hash(&mut h);
                    if let Some(val) = trans.get(*key) {
                        val.hash(&mut h);
                    }
                }
            }
        }
        h.finish()
    };

    let entries = match std::fs::read_dir(dir_path) {
        Ok(e) => e,
        Err(_) => return Ok((vec![], vec![])),
    };

    // ── 第一遍：文件扫描 + AST 提取 ──
    #[derive(Clone)]
    struct FileData {
        path: PathBuf,
        stem: String,
        abs_dir: String, // 父目录绝对路径（转 slash），用于虚拟模块 ID 使相对导入可解析
        mod_source: String,
        dyn_source: String,
        func_ids: Vec<String>,
    }

    let mut fresh_files: Vec<FileData> = Vec::new();
    let mut cached_static: Vec<(String, String)> = Vec::new();
    let mut cached_dynamic: Vec<(String, String, Vec<String>)> = Vec::new();

    let mut seen_stems = std::collections::HashSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let path = if path.is_dir() {
            let ts = path.join("index.ts");
            let js = path.join("index.js");
            if ts.exists() {
                ts
            } else if js.exists() {
                js
            } else {
                continue;
            }
        } else {
            path
        };

        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let file_name = path.file_name().unwrap().to_string_lossy();
        if !(ext == "js" || ext == "ts")
            || file_name.starts_with('_')
            || file_name.ends_with(".d.ts")
        {
            continue;
        }

        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file_path_str = sugg_core::path_to_slash(&path);
        let abs_dir = sugg_core::path_to_slash(path.parent().unwrap_or(Path::new("")));
        let raw_stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let file_stem = if raw_stem == "index" {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw_stem.into_owned())
        } else {
            raw_stem.into_owned()
        };

        if !seen_stems.insert(file_stem.clone()) {
            log_warn!(
                "Skipping {}: stem '{}' conflicts with another script",
                file_path_str,
                file_stem
            );
            continue;
        }

        let content_hash = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            source.hash(&mut h);
            i18n_hash.hash(&mut h);
            h.finish()
        };

        // 全缓存命中：直接复用 static_bundle + dyn_bundle（含 deps 校验）
        if let Some(cached) = cache.get(&path)
            && cached.content_hash == content_hash
            && cached.static_bundle.is_some()
            && cached.dyn_bundle.is_some()
            && deps_unchanged(&cached.deps)
        {
            println!("   {} (cached)", file_path_str);
            cached_static.push((cached.stem.clone(), cached.static_bundle.clone().unwrap()));
            cached_dynamic.push((
                cached.stem.clone(),
                cached.dyn_bundle.clone().unwrap(),
                cached.func_ids.clone(),
            ));
            continue;
        }

        println!("   {}", file_path_str);
        let result = ast::extract_dynamics(&source, &file_path_str);

        let deps = {
            let mut d = HashMap::new();
            for dep_path in ast::extract_local_imports(&source, &file_path_str) {
                let dep_hash = std::fs::read_to_string(&dep_path)
                    .ok()
                    .map(|s| {
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        s.hash(&mut h);
                        h.finish()
                    })
                    .unwrap_or(0);
                d.insert(PathBuf::from(dep_path), dep_hash);
            }
            d
        };

        cache.insert(
            path.clone(),
            CachedFile {
                content_hash,
                stem: file_stem.clone(),
                mod_source: result.0.clone(),
                dyn_source: result.1.clone(),
                func_ids: result.2.clone(),
                deps,
                static_bundle: None,
                command_node: None,
                dyn_bundle: None,
                bytecode: None,
            },
        );

        fresh_files.push(FileData {
            path,
            stem: file_stem,
            abs_dir: abs_dir.clone(),
            mod_source: result.0,
            dyn_source: result.1,
            func_ids: result.2,
        });
    }

    let total_files = fresh_files.len() + cached_static.len();
    if total_files == 0 {
        return Ok((vec![], vec![]));
    }

    let i18n_modules = generate_i18n_modules(&translations_by_ns);
    let env_code = generate_env_code("");

    // ── 第二遍：并发打包 fresh 文件的 static 和 dynamic ──
    let mut static_set = tokio::task::JoinSet::new();
    for file in &fresh_files {
        let v_stat = format!("{}/__v_stat_{}.ts", file.abs_dir, file.stem);
        let entry_id = format!("{}/__v_sentry_{}.ts", file.abs_dir, file.stem);
        let mut modules = HashMap::new();
        modules.insert(v_stat.clone(), file.mod_source.clone());
        modules.insert(
            entry_id.clone(),
            generate_per_file_entry(&file.stem, &v_stat),
        );

        let env = env_code.clone();
        let i18n = i18n_modules.clone();
        let stem = file.stem.clone();
        static_set.spawn(async move {
            let code = bundle_virtual(&entry_id, modules, env, i18n)
                .await
                .with_context(|| format!("Failed to bundle static '{stem}'"))?;
            Ok::<_, anyhow::Error>((stem, code))
        });
    }

    let mut fresh_static_map: HashMap<String, String> = HashMap::new();
    while let Some(res) = static_set.join_next().await {
        let (stem, code) = res.context("Static bundle join task failed")??;
        fresh_static_map.insert(stem.clone(), code);
    }
    // 写回 cache
    for file in &fresh_files {
        if let Some(code) = fresh_static_map.get(&file.stem)
            && let Some(cached) = cache.get_mut(&file.path)
        {
            cached.static_bundle = Some(code.clone());
        }
    }

    let mut dyn_set = tokio::task::JoinSet::new();
    for file in &fresh_files {
        let v_dyn = format!("{}/__v_dyn_{}.ts", file.abs_dir, file.stem);
        let mut modules = HashMap::new();
        modules.insert(v_dyn.clone(), file.dyn_source.clone());
        modules.insert(
            VIRTUAL_DYNAMIC_ENTRY.to_string(),
            format!("export * from '{}';\n", v_dyn),
        );

        let dyn_env_code = generate_env_code(&file.stem);
        let i18n = i18n_modules.clone();
        let stem = file.stem.clone();
        let func_ids = file.func_ids.clone();
        dyn_set.spawn(async move {
            let code = bundle_virtual(VIRTUAL_DYNAMIC_ENTRY, modules, dyn_env_code, i18n)
                .await
                .with_context(|| format!("Failed to bundle dynamic '{stem}'"))?;
            Ok::<_, anyhow::Error>((stem, code, func_ids))
        });
    }

    let mut fresh_dynamic_map: HashMap<String, (String, Vec<String>)> = HashMap::new();
    while let Some(res) = dyn_set.join_next().await {
        let (stem, code, func_ids) = res.context("Dynamic bundle join task failed")??;
        fresh_dynamic_map.insert(stem.clone(), (code, func_ids));
    }
    // 写回 cache
    for file in &fresh_files {
        if let Some((code, _)) = fresh_dynamic_map.get(&file.stem)
            && let Some(cached) = cache.get_mut(&file.path)
        {
            cached.dyn_bundle = Some(code.clone());
        }
    }

    // ── 组装最终结果（cached + fresh） ──
    let mut static_results: Vec<(String, String)> = cached_static;
    for file in &fresh_files {
        if let Some(code) = fresh_static_map.get(&file.stem) {
            static_results.push((file.stem.clone(), code.clone()));
        }
    }

    let mut dynamic_results: Vec<(String, String, Vec<String>)> = cached_dynamic;
    for file in &fresh_files {
        if let Some((code, func_ids)) = fresh_dynamic_map.get(&file.stem) {
            dynamic_results.push((file.stem.clone(), code.clone(), func_ids.clone()));
        }
    }

    Ok((static_results, dynamic_results))
}

/// 扫描 i18n 目录下的 JSON 文件，只返回 file_stem 匹配回退链（忽略大小写）的 `(stem, path)` 列表。
pub fn get_matching_i18n_files(
    i18n_dir: &Path,
    fallbacks: &[String],
) -> Vec<(String, std::path::PathBuf)> {
    let mut existing_files: Vec<(String, std::path::PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(i18n_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                existing_files.push((stem.to_string(), path));
            }
        }
    }

    let mut results = Vec::new();
    for fb in fallbacks {
        if let Some((stem, path)) = existing_files
            .iter()
            .find(|(stem, _)| stem.eq_ignore_ascii_case(fb))
        {
            results.push((stem.clone(), path.clone()));
        }
    }
    results
}

/// 扫描 `completions_dir` 下的所有子命令 i18n 目录，返回 `(namespace, i18n_dir_path)` 列表。
pub fn scan_i18n_dirs(completions_dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(completions_dir) {
        let mut subdirs: Vec<(String, std::path::PathBuf)> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect();
        subdirs.sort_by(|a, b| a.0.cmp(&b.0));
        for (dirname, path) in &subdirs {
            if dirname == "i18n" {
                continue;
            }
            let sub_i18n = path.join("i18n");
            if sub_i18n.is_dir() {
                result.push((dirname.clone(), sub_i18n));
            }
        }
    }
    result
}
