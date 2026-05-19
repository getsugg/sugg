//! Sugg CLI - 智能命令行自动完成工具
//!
//! 模块结构：
//! - `cache`: 缓存数据结构与序列化
//! - `ast`: AST 提取与分析
//! - `bundler`: 虚拟模块插件与打包
//! - `js`: JavaScript 运行时与辅助函数

pub mod ast;
pub mod bundler;
pub mod cache;
pub mod js;
pub mod logger;

/// 支持的 Shell 类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shell {
    Nushell,
    Zsh,
    Fish,
    Bash,
    Powershell,
}

impl std::str::FromStr for Shell {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "nushell" | "nu" => Ok(Shell::Nushell),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            "bash" => Ok(Shell::Bash),
            "powershell" | "pwsh" => Ok(Shell::Powershell),
            _ => Err(format!(
                "Unsupported shell: '{}'. Supported shells: nushell, zsh, fish, bash, powershell",
                s
            )),
        }
    }
}

use std::collections::HashMap;
use std::path::Path;

/// 扫描 `dir_path` 下的补全脚本，用指定 `lang` 打包，返回 `(bundle_static, bundle_dynamic)`。
pub async fn build_bundles(
    dir_path: &Path,
    lang: &str,
) -> anyhow::Result<(String, Vec<(String, String, Vec<String>)>)> {
    use bundler::{VIRTUAL_DYNAMIC_ENTRY, VIRTUAL_STATIC_ENTRY, bundle_virtual};
    use js::codegen::{generate_env_code, generate_i18n_modules, generate_import_stmt};

    // 先扫描翻译，确定命名空间列表，再生成 import stmt
    let load_json = |p: &Path| -> HashMap<String, String> {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };
    let mut translations_by_ns: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (ns, i18n_dir) in scan_i18n_dirs(dir_path) {
        let lp = i18n_dir.join(format!("{}.json", lang));
        let ep = i18n_dir.join("en.json");
        let mut map = HashMap::new();
        if ep.exists() {
            map.extend(load_json(&ep));
        }
        if lp.exists() && lang != "en" {
            map.extend(load_json(&lp));
        }
        translations_by_ns.insert(ns, map);
    }

    let import_stmt = generate_import_stmt();

    let mut virtual_statics = HashMap::new();
    let mut virtual_dynamics: Vec<(String, HashMap<String, String>, Vec<String>)> = Vec::new();
    let mut static_entry = import_stmt.clone();
    static_entry.push_str("export { __parseConfig };\n");
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
        let file_path_str = path_to_slash(&path);
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
        let abs_dir = path_to_slash(path.parent().unwrap_or(Path::new("")));

        let v_stat = format!("{}/__v_stat_{}.ts", abs_dir, file_stem);
        let mut stat_code = import_stmt.clone();
        stat_code.push_str(&mod_source);
        virtual_statics.insert(v_stat.clone(), stat_code);
        static_entry.push_str(&format!("import c{} from '{}';\n", idx, v_stat));
        config_merges.push_str(&format!("['{}', c{}], ", file_stem, idx));

        let v_dyn = format!("{}/__v_dyn_{}.ts", abs_dir, file_stem);
        let mut dyn_modules = HashMap::new();
        let mut dyn_code = import_stmt.clone();
        dyn_code.push_str(&dyn_source);
        dyn_modules.insert(v_dyn.clone(), dyn_code);
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
    let env_code = generate_env_code(lang);
    let i18n_modules = generate_i18n_modules(&translations_by_ns);
    let s = bundle_virtual(
        VIRTUAL_STATIC_ENTRY,
        virtual_statics,
        env_code.clone(),
        i18n_modules.clone(),
    )
    .await?;
    let mut dynamic_bundles = Vec::new();
    for (stem, modules, func_ids) in virtual_dynamics {
        let d = bundle_virtual(
            VIRTUAL_DYNAMIC_ENTRY,
            modules,
            env_code.clone(),
            i18n_modules.clone(),
        )
        .await?;
        dynamic_bundles.push((stem, d, func_ids));
    }
    Ok((s, dynamic_bundles))
}

/// 扫描 `completions_dir` 下的所有子命令 i18n 目录，返回 `(namespace, i18n_dir_path)` 列表。
/// 每个子命令的 i18n 目录对应 `completions_dir/{subdir}/i18n/`。
pub fn scan_i18n_dirs(completions_dir: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
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

/// 统一的 sugg 根目录（可通过 SUGG_HOME 覆盖）
pub fn sugg_root() -> std::path::PathBuf {
    if let Ok(var) = std::env::var("SUGG_HOME") {
        return std::path::PathBuf::from(var);
    }
    dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("sugg")
}

/// 缓存文件路径
pub fn get_cache_path() -> std::path::PathBuf {
    let dir = sugg_root();
    std::fs::create_dir_all(&dir).ok();
    dir.join(".completion_cache.bin")
}

/// 默认补全脚本目录
pub fn default_completions_dir() -> std::path::PathBuf {
    sugg_root().join("completions")
}

/// 将路径转换为统一的正斜杠字符串（跨平台安全）
pub fn path_to_slash(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 错误日志宏
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Error, &msg);
    }};
}

/// 警告日志宏
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Warn, &msg);
    }};
}

/// 信息日志宏
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Info, &msg);
    }};
}
