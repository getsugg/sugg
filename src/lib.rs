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

impl Shell {
    pub fn as_str(&self) -> &'static str {
        match self {
            Shell::Nushell => "nushell",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Bash => "bash",
            Shell::Powershell => "powershell",
        }
    }
}

use icu::locale::Locale;
use icu::locale::fallback::LocaleFallbacker;
use std::collections::HashMap;
use std::path::Path;

/// 基于 BCP 47 规范生成语言回退链（使用 ICU4X 标准实现）
///
/// # 示例
/// ```
/// # use sugg::get_fallback_chain;
/// // en 始终兜底，"en-US" 等返回 ["en", "en-US"]（en-US.json 不存在时 build_bundles 自然跳过）
/// assert_eq!(get_fallback_chain("en"),        vec!["en"]);
/// assert_eq!(get_fallback_chain("en-US"),     vec!["en", "en-US"]);
/// // zh-Hans-CN 依次回退: en → zh → zh-CN（脚本被 likelySubtags 最小化）
/// assert_eq!(get_fallback_chain("zh-Hans-CN"), vec!["en", "zh", "zh-CN"]);
/// // zh-Hant-TW: en → zh-Hant → zh-TW（脚本被最小化后通过 max_script 补充回来）
/// assert_eq!(get_fallback_chain("zh-Hant-TW"), vec!["en", "zh-Hant", "zh-TW"]);
/// // 纯双字母语言码: en → fr → fr-FR
/// assert_eq!(get_fallback_chain("fr-FR"),     vec!["en", "fr", "fr-FR"]);
/// // 带变体的语言标签正确处理
/// assert_eq!(get_fallback_chain("zh-Hans-CN-pinyin"), vec!["en", "zh", "zh-pinyin", "zh-CN", "zh-CN-pinyin"]);
/// assert_eq!(get_fallback_chain("en-US-posix"),       vec!["en", "en-posix", "en-US", "en-US-posix"]);
/// ```
pub fn get_fallback_chain(lang: &str) -> Vec<String> {
    let mut chain = vec!["en".to_string()];
    if lang.is_empty() || lang.eq_ignore_ascii_case("en") {
        return chain;
    }

    if let Ok(locale) = lang.parse::<Locale>() {
        let fallbacker = LocaleFallbacker::new();
        // 默认配置已包含语言、脚本、区域和变体的完整回退
        let mut iter = fallbacker
            .for_config(Default::default())
            .fallback_for(locale.into());

        let mut sequence = Vec::new();
        loop {
            let s = iter.get().to_string();
            if s == "und" {
                break;
            }
            if s != "en" {
                sequence.push(s);
            }
            iter.step();
        }
        // sequence 现在是从最特化到最泛化的列表
        // 如 ["zh-Hans-CN-pinyin", "zh-Hans-CN", "zh-Hans", "zh"]
        // 反转后得到泛化在前、特化在后的顺序，方便后续 map.extend 覆盖
        sequence.reverse();
        for s in sequence {
            if !chain.contains(&s) {
                chain.push(s);
            }
        }
    } else {
        // 无法解析时保守处理：只加入原串
        if !lang.eq_ignore_ascii_case("en") {
            chain.push(lang.to_string());
        }
    }

    chain
}

/// 扫描 `dir_path` 下的补全脚本，用指定 `lang` 打包，返回 `(bundle_static, bundle_dynamic)`。
pub async fn build_bundles(
    dir_path: &Path,
    lang: &str,
) -> anyhow::Result<(String, Vec<(String, String, Vec<String>)>)> {
    use anyhow::Context;
    use bundler::{VIRTUAL_DYNAMIC_ENTRY, VIRTUAL_STATIC_ENTRY, bundle_virtual};
    use js::codegen::{generate_env_code, generate_i18n_modules};

    let load_json = |p: &Path| -> HashMap<String, String> {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    };
    let mut translations_by_ns: HashMap<String, HashMap<String, String>> = HashMap::new();
    for (ns, i18n_dir) in scan_i18n_dirs(dir_path) {
        // 生成标准 BCP 47 回退链并依序加载
        // 顺序：en → zh → zh-Hans → zh-Hans-CN
        // map.extend 的迭代顺序保证后加载的特化文件覆盖同 key 的泛化翻译
        let fallbacks = get_fallback_chain(lang);

        // 统一使用 get_matching_i18n_files 扫描并按回退链匹配（忽略大小写）
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
    let env_code = generate_env_code(lang);
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
        let d = bundle_virtual(
            VIRTUAL_DYNAMIC_ENTRY,
            modules,
            env_code.clone(),
            i18n_modules.clone(),
        )
        .await
        .with_context(|| format!("Failed to bundle '{stem}'"))?;
        dynamic_bundles.push((stem, d, func_ids));
    }
    Ok((s, dynamic_bundles))
}

/// 扫描 i18n 目录下的 JSON 文件，只返回 file_stem 匹配回退链（忽略大小写）的 `(stem, path)` 列表。
/// 按回退链顺序排列，确保 `build_bundles` 和 `run_i18n_gen` 使用相同的匹配逻辑。
pub fn get_matching_i18n_files(
    i18n_dir: &std::path::Path,
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

use std::sync::OnceLock;

/// 终端是否支持富文本 Emoji
pub fn use_emoji() -> bool {
    *USE_EMOJI.get_or_init(|| {
        if std::env::var("NO_COLOR").is_ok() {
            return false;
        }
        #[cfg(target_os = "windows")]
        {
            std::env::var("WT_SESSION").is_ok()
                || std::env::var("TERM_PROGRAM")
                    .map(|v| v == "vscode")
                    .unwrap_or(false)
                || std::env::var("COLORTERM").is_ok()
        }
        #[cfg(not(target_os = "windows"))]
        {
            true
        }
    })
}

static USE_EMOJI: OnceLock<bool> = OnceLock::new();

/// 智能 Emoji 包装器：实现 Display，自动根据环境降级
#[derive(Clone, Copy, Debug)]
pub struct Emoji {
    pub rich: &'static str,
    pub fallback: &'static str,
}

impl Emoji {
    pub const fn new(rich: &'static str, fallback: &'static str) -> Self {
        Self { rich, fallback }
    }
}

impl std::fmt::Display for Emoji {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if use_emoji() {
            f.write_str(self.rich)
        } else {
            f.write_str(self.fallback)
        }
    }
}

// =========================================================================
// 全局统一的 CLI 符号字典
// =========================================================================
pub const ICON_SUCCESS: Emoji = Emoji::new("✅", "√");
pub const ICON_ERROR: Emoji = Emoji::new("❌", "×");
pub const ICON_WARN: Emoji = Emoji::new("❗", "!");
pub const ICON_INFO: Emoji = Emoji::new("💡", "i");
pub const ICON_LOG: Emoji = Emoji::new("📝", "-");
pub const ICON_BUILD: Emoji = Emoji::new("🛠️", "*");
pub const ICON_PACKAGE: Emoji = Emoji::new("📦", "o");
pub const ICON_SCAN: Emoji = Emoji::new("🔍", "»");
pub const ICON_DOWNLOAD: Emoji = Emoji::new("📥", "↓");
pub const ICON_UPGRADE: Emoji = Emoji::new("⬆️", "↑");
pub const ICON_SYNC: Emoji = Emoji::new("🔄", "~");
pub const ICON_STAR: Emoji = Emoji::new("⭐", "*");
pub const ICON_ROCKET: Emoji = Emoji::new("🚀", ">");
pub const ICON_TAG: Emoji = Emoji::new("🏷️", "@");
pub const ICON_PARTY: Emoji = Emoji::new("🎉", "*");

// =========================================================================
// ANSI 颜色码（复用 use_emoji 的终端检测，NO_COLOR 时一并静默）
// =========================================================================

/// ANSI 颜色/样式码包装器：实现 Display，终端不支持时输出空字符串
pub struct Ansi(pub &'static str);

impl std::fmt::Display for Ansi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if use_emoji() {
            write!(f, "\x1b[{}m", self.0)
        } else {
            Ok(())
        }
    }
}

pub const ANSI_GREEN: Ansi = Ansi("1;32");
pub const ANSI_CYAN: Ansi = Ansi("1;36");
pub const ANSI_YELLOW: Ansi = Ansi("33");
pub const ANSI_BOLD: Ansi = Ansi("1");
pub const ANSI_RESET: Ansi = Ansi("0");

#[cfg(test)]
mod fallback_tests {
    use super::*;

    /// 边界情况：en 兜底、空字符串、非 BCP 47 格式兜底
    #[test]
    fn test_fallback_chain_edge_cases() {
        // en 始终兜底
        assert_eq!(get_fallback_chain("en"), vec!["en"]);
        // 空字符串直接返回 ["en"]
        assert_eq!(get_fallback_chain(""), vec!["en"]);
        // 无法解析的字符串：["en"] + 原字符串，不生成多余的 en-* 衍生项
        let chain = get_fallback_chain("???");
        assert!(chain.contains(&"en".to_string()));
        assert!(chain.contains(&"???".to_string()));
        let en_prefix: Vec<_> = chain.iter().filter(|s| s.starts_with("en")).collect();
        assert_eq!(en_prefix, vec![&"en"], "不应生成 en-* 衍生项");
    }
}
