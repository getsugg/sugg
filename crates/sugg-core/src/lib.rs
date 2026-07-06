//! Sugg Core - 核心公共库
//!
//! 提供 CLI 和 Engine 共同依赖的基础设施：
//! - `cache`: 缓存数据结构与序列化（rkyv 零拷贝）
//! - `js`: JavaScript 运行时注入与代码生成
//! - `logger`: 统一日志系统

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

/// 统一的 sugg 根目录：~/.sugg（可通过 SUGG_HOME 覆盖）
pub fn sugg_root() -> std::path::PathBuf {
    if let Ok(var) = std::env::var("SUGG_HOME") {
        return std::path::PathBuf::from(var);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".sugg")
}

/// 缓存文件路径
pub fn get_cache_path() -> std::path::PathBuf {
    let dir = sugg_root().join("cache");
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

        let terminal_width = console::Term::stderr().size().1 as usize;
        let effective_width = if terminal_width > 20 {
            max_width.min(terminal_width.saturating_sub(12))
        } else {
            max_width
        };

        let border = self.border_style.clone().for_stderr();
        let horizontal = "─".repeat(effective_width + 4); // 左右各留 2 格内边距

        eprintln!();
        eprintln!("{}", border.apply_to(format!("  ╭{}╮", horizontal)));

        for line in &self.lines {
            let padded = console::pad_str(line, effective_width, console::Alignment::Left, None);
            eprintln!("    {}", padded); // 2 框缩进 + 2 内边距
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
mod display_tests {
    use super::*;

    /// 验证 Style(for_stderr) 在 force_styling 下的行为
    ///
    /// 用 `force_styling` 而非全局 `set_colors_enabled_stderr`：
    /// - 避免依赖 `console::colors_enabled_stderr()` 的全局状态（cargo test 并行测试会竞争）
    /// - 避免依赖 stderr 实际是否为真实终端（cargo test 抓取 stderr 时不是 tty）
    /// - `force_styling(true)` 强制覆盖所有自动检测
    #[test]
    fn test_style_color_on_off_stderr() {
        // ===== 关闭 =====
        let off = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(false);
        let output = off.apply_to("hello").to_string();
        assert!(
            !output.contains('\x1b'),
            "force_styling(false) 时应抑制 ANSI 码: {:?}",
            output
        );
        assert_eq!(output, "hello", "force_styling(false) 时应输出纯文本");

        // ===== 开启 =====
        let on = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(true);
        let output = on.apply_to("hello").to_string();
        assert!(
            output.contains('\x1b'),
            "force_styling(true) 时应包含 ANSI 码"
        );
        assert!(output.starts_with("\x1b["), "应以 ANSI 转义序列开头");
        assert!(output.ends_with("\x1b[0m"), "应以 ANSI 重置结尾");
        assert!(output.contains("hello"), "应保留原始文本");
    }

    /// 验证 for_stdout / for_stderr 是独立的输出目标标记
    ///
    /// 同样用 `force_styling` 解耦：每侧显式控制颜色开关，断言两侧输出按各自配置走。
    #[test]
    fn test_for_stdout_independent_from_stderr() {
        // 组合 1：stderr 关、stdout 开
        let stderr_off = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(false);
        let stdout_on = console::Style::new()
            .bold()
            .cyan()
            .for_stdout()
            .force_styling(true);
        assert!(
            !stderr_off.apply_to("x").to_string().contains('\x1b'),
            "stderr force_styling(false) 应抑制 ANSI 码"
        );
        assert!(
            stdout_on.apply_to("x").to_string().contains('\x1b'),
            "stdout force_styling(true) 应输出 ANSI 码"
        );

        // 组合 2：stderr 开、stdout 关
        let stderr_on = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(true);
        let stdout_off = console::Style::new()
            .bold()
            .cyan()
            .for_stdout()
            .force_styling(false);
        assert!(
            stderr_on.apply_to("x").to_string().contains('\x1b'),
            "stderr force_styling(true) 应输出 ANSI 码"
        );
        assert!(
            !stdout_off.apply_to("x").to_string().contains('\x1b'),
            "stdout force_styling(false) 应抑制 ANSI 码"
        );
    }

    /// 验证 Emoji 常量至少能渲染出内容（不 panic）
    #[test]
    fn test_emoji_constants_render() {
        let s = ICON_SUCCESS.to_string();
        assert!(!s.is_empty(), "ICON_SUCCESS 应渲染出内容");
        let s = ICON_ERROR.to_string();
        assert!(!s.is_empty(), "ICON_ERROR 应渲染出内容");
        let s = ICON_PARTY.to_string();
        assert!(!s.is_empty(), "ICON_PARTY 应渲染出内容");
    }
}
