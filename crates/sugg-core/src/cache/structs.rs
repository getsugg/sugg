use rkyv::{Archive, Deserialize, Serialize};

pub fn get_cache_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUGG_CACHE_DIR") {
        let path = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&path).expect("Failed to create cache directory");
        return path.join(".completion_cache.bin");
    }
    crate::get_cache_path()
}

/// 补全建议的显示样式，与 TypeScript SuggestionStyle 一致
#[derive(
    Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Serialize, serde::Deserialize,
)]
#[rkyv(attr(derive(Debug)))]
pub struct SuggestionStyle {
    /// 前景色
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// 背景色
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// 文本属性：bold, italic, underline, dim
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<Vec<String>>,
}

/// 用于存储 args 中的静态数组补全
#[derive(
    Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Serialize, serde::Deserialize,
)]
#[rkyv(attr(derive(Debug)))]
pub struct StaticSuggestion {
    pub display: String,
    pub value: String,
    pub description: String,
    pub style: Option<SuggestionStyle>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Deserialize)]
#[rkyv(attr(derive(Debug)))]
pub struct OptionItem {
    pub labels: Vec<String>,
    pub description: String,
    pub style: Option<SuggestionStyle>,
    pub takes_value: bool,
    pub dynamic_func: Option<String>,
    pub static_args: Option<Vec<StaticSuggestion>>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Deserialize)]
#[rkyv(attr(derive(Debug)))]
#[rkyv(serialize_bounds(
    __S: rkyv::ser::Writer + rkyv::ser::Allocator,
    __S::Error: rkyv::rancor::Source,
))]
#[rkyv(deserialize_bounds(__D::Error: rkyv::rancor::Source))]
#[rkyv(bytecheck(bounds(
    __C: rkyv::validation::ArchiveContext,
    <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source,
)))]
pub struct CommandNode {
    pub name: String,
    pub description: String,
    pub style: Option<SuggestionStyle>,
    pub target: Option<u32>,
    #[rkyv(omit_bounds)]
    pub subcommands: Vec<CommandNode>,
    pub options: Vec<OptionItem>,
    pub dynamic_func: Option<String>,
    pub static_args: Option<Vec<StaticSuggestion>>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone)]
#[rkyv(attr(derive(Debug)))]
pub struct CompletionCache {
    pub root: CommandNode,
    /// 每个脚本的 dynamic bundle bytecode，按脚本扫描顺序排列
    pub bytecodes: Vec<Vec<u8>>,
    /// 动态函数 id → bytecodes 下标，用于运行时按函数名定位 module
    pub dyn_index: Vec<(String, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_cache_path() {
        let path = get_cache_path();
        assert!(path.is_absolute());
        assert!(path.ends_with(".completion_cache.bin"));
        assert!(path.parent().unwrap().exists());
    }
}
