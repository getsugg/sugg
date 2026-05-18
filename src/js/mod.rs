pub mod codegen;
pub mod runtime;

pub use codegen::{generate_env_code, generate_i18n_modules, generate_import_stmt};
pub use runtime::inject_globals;
