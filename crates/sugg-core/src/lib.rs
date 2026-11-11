//! Sugg Core - core shared library
//!
//! Shared infrastructure used by both CLI and Engine:
//! - `cache`: cache data structures and serialization (rkyv zero-copy)
//! - `js`: JavaScript runtime injection and code generation
//! - `logger`: unified logging system

pub mod cache;
pub mod js;
pub mod logger;

/// Supported Shell types
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

/// Unified sugg root directory: ~/.sugg (overridable via SUGG_HOME)
pub fn sugg_root() -> std::path::PathBuf {
    if let Ok(var) = std::env::var("SUGG_HOME") {
        return std::path::PathBuf::from(var);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".sugg")
}

/// Cache file path
pub fn get_cache_path() -> std::path::PathBuf {
    let dir = sugg_root().join("cache");
    std::fs::create_dir_all(&dir).ok();
    dir.join(".completion_cache.bin")
}

/// Default completions script directory
pub fn default_completions_dir() -> std::path::PathBuf {
    sugg_root().join("completions")
}

/// Convert a path to a unified forward-slash string (cross-platform safe)
pub fn path_to_slash(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Error logging macro
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Error, &msg);
    }};
}

/// Warning logging macro
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Warn, &msg);
    }};
}

/// Info logging macro
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {{
        let msg = format!($($arg)*);
        $crate::logger::write_log($crate::logger::LogLevel::Info, &msg);
    }};
}

// =========================================================================
// Global unified CLI symbol dictionary
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
// TerminalBox - generic rounded border rendering component
// =========================================================================

/// Terminal rounded card container with adaptive width and Builder-style chaining
pub struct TerminalBox {
    lines: Vec<String>,
    border_style: console::Style,
}

impl TerminalBox {
    /// Create a new TerminalBox; border color defaults to cyan
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            border_style: console::Style::new().bold().cyan(),
        }
    }

    /// Set the border color
    pub fn border_color(mut self, style: console::Style) -> Self {
        self.border_style = style;
        self
    }

    /// Append a line of content
    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(text.into());
        self
    }

    /// Append an empty line
    pub fn empty_line(mut self) -> Self {
        self.lines.push(String::new());
        self
    }

    /// Render and print the box to stderr
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
        let horizontal = "─".repeat(effective_width + 4); // 2 columns of padding on each side

        eprintln!();
        eprintln!("{}", border.apply_to(format!("  ╭{}╮", horizontal)));

        for line in &self.lines {
            let padded = console::pad_str(line, effective_width, console::Alignment::Left, None);
            eprintln!("    {}", padded); // 2 frame indent + 2 padding
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

    /// Verify Style(for_stderr) behavior under force_styling
    ///
    /// Use `force_styling` instead of global `set_colors_enabled_stderr`:
    /// - Avoid depending on the global state of `console::colors_enabled_stderr()` (parallel cargo tests race)
    /// - Avoid depending on whether stderr is a real terminal (it is not a tty when cargo test captures stderr)
    /// - `force_styling(true)` forcibly overrides all auto-detection
    #[test]
    fn test_style_color_on_off_stderr() {
        // ===== off =====
        let off = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(false);
        let output = off.apply_to("hello").to_string();
        assert!(
            !output.contains('\x1b'),
            "force_styling(false) should suppress ANSI codes: {:?}",
            output
        );
        assert_eq!(output, "hello", "force_styling(false) yields plain text");

        // ===== on =====
        let on = console::Style::new()
            .bold()
            .cyan()
            .for_stderr()
            .force_styling(true);
        let output = on.apply_to("hello").to_string();
        assert!(
            output.contains('\x1b'),
            "force_styling(true) should include ANSI codes"
        );
        assert!(output.starts_with("\x1b["), "should start with ANSI escape");
        assert!(output.ends_with("\x1b[0m"), "should end with an ANSI reset");
        assert!(output.contains("hello"), "should keep the original text");
    }

    /// Verify that for_stdout / for_stderr are independent output target markers
    ///
    /// Also decoupled via `force_styling`: each side controls colors explicitly and follows its own config.
    #[test]
    fn test_for_stdout_independent_from_stderr() {
        // Combination 1: stderr off, stdout on
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
            "stderr force_styling(false) should suppress ANSI codes"
        );
        assert!(
            stdout_on.apply_to("x").to_string().contains('\x1b'),
            "stdout force_styling(true) should output ANSI codes"
        );

        // Combination 2: stderr on, stdout off
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
            "stderr force_styling(true) should output ANSI codes"
        );
        assert!(
            !stdout_off.apply_to("x").to_string().contains('\x1b'),
            "stdout force_styling(false) should suppress ANSI codes"
        );
    }

    /// Verify that Emoji constants render at least some content (no panic)
    #[test]
    fn test_emoji_constants_render() {
        let s = ICON_SUCCESS.to_string();
        assert!(!s.is_empty(), "ICON_SUCCESS should render content");
        let s = ICON_ERROR.to_string();
        assert!(!s.is_empty(), "ICON_ERROR should render content");
        let s = ICON_PARTY.to_string();
        assert!(!s.is_empty(), "ICON_PARTY should render content");
    }
}
