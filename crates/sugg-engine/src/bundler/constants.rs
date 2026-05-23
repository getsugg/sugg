//! 虚拟模块标识符常量，统一管理避免拼写错误

/// 环境虚拟模块标识符
pub const VIRTUAL_ENV: &str = "virtual:env";

/// i18n 翻译常量虚拟模块标识符
pub const VIRTUAL_I18N: &str = "virtual:i18n";

/// 静态入口虚拟模块标识符
pub const VIRTUAL_STATIC_ENTRY: &str = "virtual:static_entry";

/// 动态入口虚拟模块标识符
pub const VIRTUAL_DYNAMIC_ENTRY: &str = "virtual:dynamic_entry";

/// 用于生成动态函数名的前缀
pub const DYNAMIC_FUNC_PREFIX: &str = "__dyn_";

/// 用于标记动态节点的特殊属性名
pub const IS_DYNAMIC_MARKER: &str = "__is_dynamic";

/// 动态函数在 JS 模块中的 ID 字段名
pub const DYNAMIC_ID_FIELD: &str = "id";
