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

// =========================================================================
// 全局统一的 CLI 符号字典
// =========================================================================

pub const ICON_SUCCESS: &str = "✅";
pub const ICON_ERROR: &str = "❌";
pub const ICON_WARN: &str = "❗";
pub const ICON_INFO: &str = "💡";
pub const ICON_LOG: &str = "📝";
pub const ICON_BUILD: &str = "🔧";
pub const ICON_PACKAGE: &str = "📦";
pub const ICON_SCAN: &str = "🔍";
pub const ICON_DOWNLOAD: &str = "📥";
pub const ICON_UPGRADE: &str = "🔼";
pub const ICON_SYNC: &str = "🔄";
pub const ICON_STAR: &str = "⭐";
pub const ICON_ROCKET: &str = "🚀";
pub const ICON_TAG: &str = "🔖";
pub const ICON_PARTY: &str = "🎉";
pub const ICON_SPARKLES: &str = "✨";
pub const ICON_POINTER: &str = "👉";

// =========================================================================
// TerminalBox — 通用圆角边框渲染组件
// =========================================================================

/// 终端圆角卡片容器，支持自适应宽度和 Builder 模式链式构建
pub struct TerminalBox {
    lines: Vec<String>,
    border_style: console::Style,
}

impl TerminalBox {
    /// 创建新的 TerminalBox，默认边框颜色为青色
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            border_style: console::Style::new().bold().cyan(),
        }
    }

    /// 设置边框颜色
    pub fn border_color(mut self, style: console::Style) -> Self {
        self.border_style = style;
        self
    }

    /// 追加一行内容
    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(text.into());
        self
    }

    /// 追加一个空行
    pub fn empty_line(mut self) -> Self {
        self.lines.push(String::new());
        self
    }

    /// 渲染并打印盒子到 stderr
    pub fn print(&self) {
        let max_width = self
            .lines
            .iter()
            .map(|l| console::measure_text_width(l))
            .max()
            .unwrap_or(0);

        let left_padding = 2;
        let right_padding = 2;
        let total_padding = left_padding + right_padding;

        // 动态适配终端宽度：获取 stderr 的物理列宽，防止窄终端溢出
        let terminal_width = console::Term::stderr().size().1 as usize;
        let effective_width = if terminal_width > 20 {
            max_width.min(terminal_width.saturating_sub(12))
        } else {
            max_width
        };

        // 使用 for_stderr() 确保颜色检测使用 stderr 而非 stdout
        let border = self.border_style.clone().for_stderr();

        let horizontal = "─".repeat(effective_width + total_padding);

        // 整个边框行作为一个样式化整体包裹
        eprintln!();
        eprintln!("{}", border.apply_to(format!("  ╭{}╮", horizontal)));

        for line in &self.lines {
            let padded = console::pad_str(line, effective_width, console::Alignment::Left, None);

            eprintln!(
                "  {}  {}  {}",
                border.apply_to("│"),
                padded,
                border.apply_to("│"),
            );
        }

        eprintln!("{}", border.apply_to(format!("  ╰{}╯", horizontal)));
        eprintln!();
    }
}

impl Default for TerminalBox {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod display_tests {
    use super::*;

    /// 验证 Style(for_stderr) 在颜色开关下的行为
    /// 注意：set_colors_enabled_stderr 是全局状态，并行测试会互相干扰，
    /// 所以把开/关两个场景合并到一个测试中顺序执行。
    #[test]
    fn test_style_color_on_off_stderr() {
        let prev = console::colors_enabled_stderr();

        // ===== 关闭颜色 =====
        console::set_colors_enabled_stderr(false);
        let style = console::Style::new().bold().cyan().for_stderr();
        let output = style.apply_to("hello").to_string();
        assert!(
            !output.contains('\x1b'),
            "颜色关闭时不应包含 ANSI 码: {:?}",
            output
        );
        assert_eq!(output, "hello", "颜色关闭时应输出纯文本");

        // ===== 开启颜色 =====
        console::set_colors_enabled_stderr(true);
        let style = console::Style::new().bold().cyan().for_stderr();
        let output = style.apply_to("hello").to_string();
        assert!(output.contains('\x1b'), "颜色开启时应包含 ANSI 码");
        assert!(output.starts_with("\x1b["), "应以 ANSI 转义序列开头");
        assert!(output.ends_with("\x1b[0m"), "应以 ANSI 重置结尾");
        assert!(output.contains("hello"), "应保留原始文本");

        console::set_colors_enabled_stderr(prev);
    }

    /// 验证 Style（for_stdout）不受 stderr 颜色设置影响
    #[test]
    fn test_for_stdout_independent_from_stderr() {
        let prev_stderr = console::colors_enabled_stderr();
        let prev_stdout = console::colors_enabled();

        // stderr 关闭，stdout 保持原样
        console::set_colors_enabled_stderr(false);

        let stderr_style = console::Style::new().bold().cyan().for_stderr();
        let stdout_style = console::Style::new().bold().cyan().for_stdout();

        let stderr_out = stderr_style.apply_to("x").to_string();
        let stdout_out = stdout_style.apply_to("x").to_string();

        // stderr 颜色被抑制
        assert!(!stderr_out.contains('\x1b'), "stderr 不应有 ANSI 码");
        // stdout 不受影响（取决于实际环境，这里只验证它们可以不同）
        assert_eq!(
            stdout_out.contains('\x1b'),
            prev_stdout,
            "stdout 应与之前一致"
        );

        console::set_colors_enabled_stderr(prev_stderr);
        console::set_colors_enabled(prev_stdout);
    }

    /// 验证 Emoji 常量至少能渲染出内容（不 panic）
    /// 注意：console::Emoji 内部用 OnceLock 缓存 is_emoji_enabled() 结果，
    /// 在测试中无法通过运行时改环境变量覆盖，但实际运行时 NO_COLOR/TERM=dumb
    /// 都会在第一次调用时被正确检测。console 库自身有对应测试。
    #[test]
    fn test_emoji_constants_render() {
        let s = format!("{}", ICON_SUCCESS);
        assert!(!s.is_empty(), "ICON_SUCCESS 应渲染出内容");
        let s = format!("{}", ICON_ERROR);
        assert!(!s.is_empty(), "ICON_ERROR 应渲染出内容");
        let s = format!("{}", ICON_PARTY);
        assert!(!s.is_empty(), "ICON_PARTY 应渲染出内容");
    }
}
