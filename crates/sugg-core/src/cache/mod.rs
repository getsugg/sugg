pub mod disk;
pub mod structs;
pub use disk::DiskCache;

pub use structs::{CommandNode, CompletionCache, OptionItem, SuggestionStyle, get_cache_path};

use crate::Shell;
use serde::Serialize;

/// 补全单项：包含值、显示文本、描述和可选的显示样式
#[derive(Debug, Clone, Serialize)]
pub struct CompletionItem {
    pub display: String,
    pub value: String,
    pub description: String,
    /// 显示样式（前景色、背景色、文本属性）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<SuggestionStyle>,
}

impl CompletionItem {
    pub fn new(display: String, description: String, style: Option<SuggestionStyle>) -> Self {
        Self {
            display: display.clone(),
            value: display,
            description,
            style,
        }
    }

    pub fn with_trailing_space(mut self) -> Self {
        self.value = format!("{} ", self.value);
        self
    }

    pub fn simple(display: String, description: String) -> Self {
        Self::new(display, description, None)
    }
}

#[derive(Serialize)]
struct NushellItem {
    value: String,
    display_override: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<SuggestionStyle>,
}

/// 打印补全结果，根据 Shell 类型选择输出格式
pub fn print_results(items: Vec<CompletionItem>, shell: &Shell) {
    match shell {
        Shell::Nushell => {
            let out: Vec<NushellItem> = items
                .into_iter()
                .map(|item| NushellItem {
                    value: item.value,
                    display_override: item.display,
                    description: item.description,
                    style: item.style,
                })
                .collect();
            if let Ok(json) = serde_json::to_string(&out) {
                println!("{}", json);
            } else {
                println!("[]");
            }
        }
        Shell::Zsh => {
            for item in items {
                let v = item.value.trim_end();
                if item.description.is_empty() {
                    println!("{}", v);
                } else {
                    println!("{}:{}", v, item.description);
                }
            }
        }
        Shell::Fish => {
            for item in items {
                let v = item.value.trim_end();
                println!("{}\t{}", v, item.description);
            }
        }
        Shell::Bash => {
            for item in items {
                println!("{}", item.value.trim_end());
            }
        }
        Shell::Powershell => {
            let out: Vec<NushellItem> = items
                .into_iter()
                .map(|item| NushellItem {
                    value: item.value,
                    display_override: item.display,
                    description: item.description,
                    style: None,
                })
                .collect();
            if let Ok(json) = serde_json::to_string(&out) {
                println!("{}", json);
            } else {
                println!("[]");
            }
        }
    }
}
