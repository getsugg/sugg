use chrono::Local;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

// 日志模式：0 = 直接输出到 stderr（Engine 使用），1 = 存入内存供 UI 展示（补全使用）
static LOG_MODE: AtomicU8 = AtomicU8::new(0);

// 专供 UI 展示的内存日志队列
static UI_LOGS: OnceLock<Mutex<Vec<(LogLevel, String)>>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
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

    // 新增：用于在 UI display 中展示简短的等级文本
    pub fn text(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERR",
            LogLevel::JsLog => "LOG",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            LogLevel::Error | LogLevel::Warn => "red",
            LogLevel::Info => "blue",
            LogLevel::JsLog => "green",
        }
    }
}

/// 设置为 UI 补全模式：所有日志不再打印到 stderr，而是以补全菜单形式呈现
pub fn set_ui_mode() {
    LOG_MODE.store(1, Ordering::SeqCst);
}

/// 统一日志写入接口：由内部 LOG_MODE 决定路由
pub fn write_log(level: LogLevel, msg: &str) {
    if LOG_MODE.load(Ordering::SeqCst) == 0 {
        // Engine 模式：直接输出到 stderr
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        eprintln!("{}[{}] {}", level.icon(), now, msg);
    } else {
        // 补全 UI 模式：拦截内容，截断后进入队列
        let mutex = UI_LOGS.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(mut guard) = mutex.lock() {
            if guard.len() < 8 {
                let short_msg = msg.lines().next().unwrap_or("").trim().to_string();
                guard.push((level, short_msg));
            }
        }
    }
}

/// 获取收集到的 UI 日志
pub fn get_ui_logs() -> Vec<(LogLevel, String)> {
    if let Some(mutex) = UI_LOGS.get() {
        if let Ok(guard) = mutex.lock() {
            return guard.clone();
        }
    }
    Vec::new()
}
