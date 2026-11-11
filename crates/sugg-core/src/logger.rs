use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

// Log mode: 0 = write directly to stderr (used by Engine), 1 = store in memory for UI display (completion)
static LOG_MODE: AtomicU8 = AtomicU8::new(0);

// In-memory log queue dedicated to UI display
static UI_LOGS: OnceLock<Mutex<Vec<(LogLevel, String)>>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Log,
}

impl LogLevel {
    pub fn icon(&self) -> &'static str {
        match self {
            LogLevel::Info => crate::ICON_INFO,
            LogLevel::Warn => crate::ICON_WARN,
            LogLevel::Error => crate::ICON_ERROR,
            LogLevel::Log => crate::ICON_LOG,
        }
    }

    pub fn text(&self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERR",
            LogLevel::Log => "LOG",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            LogLevel::Error => "red",
            LogLevel::Warn => "yellow",
            LogLevel::Info => "cyan",
            LogLevel::Log => "green",
        }
    }
}

/// Switch to UI completion mode: logs are no longer printed to stderr but rendered as a completion menu
pub fn set_ui_mode() {
    LOG_MODE.store(1, Ordering::SeqCst);
}

/// Unified log write entry point: routing is decided by the internal LOG_MODE
pub fn write_log(level: LogLevel, msg: &str) {
    if LOG_MODE.load(Ordering::SeqCst) == 0 {
        // Engine mode: write directly to stderr
        eprintln!("{} {}", level.icon(), msg);
    } else {
        // Completion UI mode: intercept content, truncate, then enqueue
        let mutex = UI_LOGS.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(mut guard) = mutex.lock()
            && guard.len() < 8
        {
            let short_msg = msg.lines().next().unwrap_or("").trim().to_string();
            guard.push((level, short_msg));
        }
    }
}

/// Get collected UI logs
pub fn get_ui_logs() -> Vec<(LogLevel, String)> {
    if let Some(mutex) = UI_LOGS.get()
        && let Ok(guard) = mutex.lock()
    {
        return guard.clone();
    }
    Vec::new()
}
