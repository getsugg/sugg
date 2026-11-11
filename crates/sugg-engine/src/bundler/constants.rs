//! Virtual module identifier constants, centrally managed to avoid typos

/// Environment virtual module identifier
pub const VIRTUAL_ENV: &str = "virtual:env";

/// User-friendly bare import name, resolved to the env virtual module
pub const VIRTUAL_SUGG: &str = "sugg";

/// i18n translation constants virtual module identifier
pub const VIRTUAL_I18N: &str = "virtual:i18n";

/// Static entry virtual module identifier
pub const VIRTUAL_STATIC_ENTRY: &str = "virtual:static_entry";

/// Dynamic entry virtual module identifier
pub const VIRTUAL_DYNAMIC_ENTRY: &str = "virtual:dynamic_entry";

// Dynamic function related constants moved to sugg-ast

pub use sugg_ast::{DYNAMIC_FUNC_PREFIX, DYNAMIC_ID_FIELD, IS_DYNAMIC_MARKER};
