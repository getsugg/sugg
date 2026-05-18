use std::path::PathBuf;
use chrono::Local;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::fs::OpenOptions;
use std::io::Write;

// 全局开关：是否将日志写入文件（默认 false，输出到控制台）
static LOG_TO_FILE: AtomicBool = AtomicBool::new(false);
// 全局缓存：记录当前的输入命令，供日志追加上下文
static CURRENT_INPUT: OnceLock<String> = OnceLock::new();

/// 开启文件日志（补全命令专享，避免 stderr 破坏终端补全的 UI）
pub fn enable_file_logging() {
    LOG_TO_FILE.store(true, Ordering::SeqCst);
}

/// 记录当前的输入，如果日志发生，会把这个上下文带上
pub fn set_current_input(input: String) {
    let _ = CURRENT_INPUT.set(input);
}

/// 日志级别，每个级别自带对应的图标
pub enum LogLevel {
    Info,
    Warn,
    Error,
    /// 专门给 JS console.log 使用的级别
    JsLog,
}

impl LogLevel {
    pub fn icon(&self) -> &'static str {
        match self {
            LogLevel::Info => "ℹ️",
            LogLevel::Warn => "⚠️",
            LogLevel::Error => "❌",
            LogLevel::JsLog => "📝",
        }
    }
}

/// 获取日志文件路径
pub fn get_log_path() -> PathBuf {
    crate::get_log_path()
}

/// 统一日志写入函数（根据运行时开关走控制台或文件）
pub fn write_log(level: LogLevel, msg: &str) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let input_ctx = CURRENT_INPUT.get().map(|i| format!(" [Input: `{}`]", i)).unwrap_or_default();
    let formatted = format!("{}[{}]{} {}", level.icon(), now, input_ctx, msg);

    if LOG_TO_FILE.load(Ordering::SeqCst) {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(crate::get_log_path()) {
            let _ = writeln!(f, "{}", formatted);
        }
    } else {
        eprintln!("{}", formatted);
    }
}
