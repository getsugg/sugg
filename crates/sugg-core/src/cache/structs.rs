use rkyv::{Archive, Deserialize, Serialize};
use rquickjs::FromJs;

pub fn get_cache_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("SUGG_CACHE_DIR") {
        let path = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&path).expect("Failed to create cache directory");
        return path.join(".completion_cache.bin");
    }
    crate::get_cache_path()
}

/// Display style for a completion suggestion, matching TypeScript SuggestionStyle
#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    FromJs,
)]
#[rkyv(attr(derive(Debug)))]
pub struct SuggestionStyle {
    /// Foreground color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// Background color
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Text attributes: bold, italic, underline, dim
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<Vec<String>>,
}

/// Stores static array completions in args
#[derive(
    Archive,
    Deserialize,
    Serialize,
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    FromJs,
)]
#[rkyv(attr(derive(Debug)))]
pub struct StaticSuggestion {
    pub display: String,
    pub value: String,
    pub description: String,
    pub style: Option<SuggestionStyle>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Deserialize, FromJs)]
#[rkyv(attr(derive(Debug)))]
pub struct OptionItem {
    pub labels: Vec<String>,
    pub description: String,
    pub style: Option<SuggestionStyle>,
    /// Tokens consumed by this option: 0 = bool option, 1 = single value (default), N = multiple values
    pub args_count: u32,
    pub dynamic_func: Option<String>,
    pub static_args: Option<Vec<StaticSuggestion>>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone, serde::Deserialize, FromJs)]
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
    /// Positional argument tokens consumed by this node: 0 = none, 1 = single value (default), N = multiple
    pub args_count: u32,
    pub dynamic_func: Option<String>,
    pub static_args: Option<Vec<StaticSuggestion>>,
}

#[derive(Archive, Deserialize, Serialize, Debug, Default, Clone)]
#[rkyv(attr(derive(Debug)))]
pub struct CompletionCache {
    pub root: CommandNode,
    /// dynamic bundle bytecode for each script, ordered by script scan order
    pub bytecodes: Vec<Vec<u8>>,
    /// dynamic function id -> bytecodes index, used at runtime to locate the module by function name
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
