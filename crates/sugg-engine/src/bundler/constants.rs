//! 虚拟模块标识符常量，统一管理避免拼写错误

/// 环境虚拟模块标识符
pub const VIRTUAL_ENV: &str = "virtual:env";

/// 用户友好的裸导入名，解析为 env 虚拟模块
pub const VIRTUAL_SUGG: &str = "sugg";

/// i18n 翻译常量虚拟模块标识符
pub const VIRTUAL_I18N: &str = "virtual:i18n";

/// 静态入口虚拟模块标识符
pub const VIRTUAL_STATIC_ENTRY: &str = "virtual:static_entry";

/// 动态入口虚拟模块标识符
pub const VIRTUAL_DYNAMIC_ENTRY: &str = "virtual:dynamic_entry";

// 动态函数相关常量已移至 sugg-ast

pub use sugg_ast::{DYNAMIC_FUNC_PREFIX, IS_DYNAMIC_MARKER, DYNAMIC_ID_FIELD};
