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
use std::path::Path;

use crate::bundler::codegen::{generate_env_code, generate_i18n_modules};
use crate::bundler::{VIRTUAL_DYNAMIC_ENTRY, VIRTUAL_STATIC_ENTRY, bundle_virtual};
use anyhow::Context;

/// 扫描 `dir_path` 下的补全脚本，用指定 `lang` 打包，返回 `(bundle_static, bundle_dynamic)`。
pub async fn build_bundles(
    dir_path: &Path,
    lang: &str,
) -> anyhow::Result<(String, Vec<(String, String, Vec<String>)>)> {
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

    let mut virtual_statics = HashMap::new();
    let mut virtual_dynamics: Vec<(String, HashMap<String, String>, Vec<String>)> = Vec::new();
    let mut static_entry =
        String::from("import { __parseConfig } from 'virtual:env';\nexport { __parseConfig };\n");
    let mut config_merges = String::new();
    let mut idx = 0;

    let entries = match std::fs::read_dir(dir_path) {
        Ok(e) => e,
        Err(_) => return Ok((String::new(), vec![])),
    };

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
        println!("   {}", file_path_str);
        let raw_stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let file_stem = if raw_stem == "index" {
            path.parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| raw_stem.into_owned())
        } else {
            raw_stem.into_owned()
        };

        let (mod_source, dyn_source, func_ids) = ast::extract_dynamics(&source, &file_path_str);
        let abs_dir = sugg_core::path_to_slash(path.parent().unwrap_or(Path::new("")));

        let v_stat = format!("{}/__v_stat_{}.ts", abs_dir, file_stem);
        virtual_statics.insert(v_stat.clone(), mod_source);
        static_entry.push_str(&format!("import c{} from '{}';\n", idx, v_stat));
        config_merges.push_str(&format!("['{}', c{}], ", file_stem, idx));

        let v_dyn = format!("{}/__v_dyn_{}.ts", abs_dir, file_stem);
        let mut dyn_modules = HashMap::new();
        dyn_modules.insert(v_dyn.clone(), dyn_source);
        dyn_modules.insert(
            VIRTUAL_DYNAMIC_ENTRY.to_string(),
            format!("export * from '{}';\n", v_dyn),
        );
        virtual_dynamics.push((file_stem.clone(), dyn_modules, func_ids));
        idx += 1;
    }

    if idx == 0 {
        return Ok((String::new(), vec![]));
    }

    static_entry.push_str(&format!("\nexport default [ {} ];\n", config_merges));
    virtual_statics.insert(VIRTUAL_STATIC_ENTRY.to_string(), static_entry);
    let env_code = generate_env_code("");
    let i18n_modules = generate_i18n_modules(&translations_by_ns);
    let s = bundle_virtual(
        VIRTUAL_STATIC_ENTRY,
        virtual_statics,
        env_code.clone(),
        i18n_modules.clone(),
    )
    .await
    .with_context(|| "Failed to bundle static entry")?;
    let mut dynamic_bundles = Vec::new();
    for (stem, modules, func_ids) in virtual_dynamics {
        let dyn_env_code = generate_env_code(&stem);
        let d = bundle_virtual(
            VIRTUAL_DYNAMIC_ENTRY,
            modules,
            dyn_env_code,
            i18n_modules.clone(),
        )
        .await
        .with_context(|| format!("Failed to bundle '{stem}'"))?;
        dynamic_bundles.push((stem, d, func_ids));
    }
    Ok((s, dynamic_bundles))
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
