/// 生成注入到每个用户脚本顶部的 import 语句。
pub fn generate_import_stmt() -> String {
    "import { createCompletion, __parseConfig } from 'virtual:env';\n".to_string()
}

/// 生成 virtual:env 模块内容。
/// 注入 `__SCRIPT_STEM` 常量，用于 `cache` API 按脚本隔离缓存命名空间。
/// 静态 bundle 传 `""`（空 stem 不前置命名空间），dynamic bundle 传对应脚本 stem。
pub fn generate_env_code(stem: &str) -> String {
    let stem_json = serde_json::to_string(stem).unwrap();
    format!(
        "const __SCRIPT_STEM = {};\n{}",
        stem_json,
        include_str!("env.js")
    )
}

/// 生成 i18n 虚拟模块代码。
/// 每个命名空间生成独立子模块 `virtual:i18n/<ns>`（平铺 export）。
/// 用户代码通过 `import { key } from 'virtual:i18n/<ns>'` 访问。
pub fn generate_i18n_modules(
    translations_by_ns: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
) -> std::collections::HashMap<String, String> {
    let mut modules = std::collections::HashMap::new();
    let mut sorted_ns: Vec<&String> = translations_by_ns
        .keys()
        .filter(|k| !k.is_empty())
        .collect();
    sorted_ns.sort();

    for ns in &sorted_ns {
        let keys = &translations_by_ns[ns.as_str()];
        let mut code = String::new();
        let mut sorted_keys: Vec<&String> = keys.keys().collect();
        sorted_keys.sort();
        for key in sorted_keys {
            let value = serde_json::to_string(&keys[key]).unwrap();
            code.push_str(&format!("export const {} = {};\n", key, value));
        }
        modules.insert(format!("virtual:i18n/{}", ns), code);
    }

    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_import_stmt() {
        let s = generate_import_stmt();
        assert!(s.contains("createCompletion"));
        assert!(s.contains("__parseConfig"));
        assert!(!s.contains("readJson"));
        assert!(s.contains("from 'virtual:env'"));
        assert!(!s.contains("virtual:i18n"));
    }

    #[test]
    fn test_generate_env_code() {
        let code = generate_env_code("test_stem");
        assert!(code.contains("const __SCRIPT_STEM = \"test_stem\""));
        assert!(code.contains("export const createCompletion"));
        assert!(code.contains("export const readJson"));
        assert!(code.contains("export const __parseConfig"));
        assert!(!code.contains("export const placeholder"));
        assert!(!code.contains("globalThis.__TRANSLATIONS"));

        let code_empty = generate_env_code("");
        assert!(code_empty.contains("const __SCRIPT_STEM = \"\""));
    }

    #[test]
    fn test_generate_i18n_modules() {
        let mut translations_by_ns = std::collections::HashMap::new();
        translations_by_ns.insert("".to_string(), std::collections::HashMap::new());

        let mut git_map = std::collections::HashMap::new();
        git_map.insert("commit".to_string(), "Commit".to_string());
        translations_by_ns.insert("git".to_string(), git_map);

        let mut docker_map = std::collections::HashMap::new();
        docker_map.insert("build".to_string(), "Build".to_string());
        translations_by_ns.insert("docker".to_string(), docker_map);

        let modules = generate_i18n_modules(&translations_by_ns);
        assert!(!modules.contains_key("virtual:i18n"));

        assert!(modules.contains_key("virtual:i18n/git"));
        assert!(modules["virtual:i18n/git"].contains("export const commit = \"Commit\";"));
        assert!(modules.contains_key("virtual:i18n/docker"));
        assert!(modules["virtual:i18n/docker"].contains("export const build = \"Build\";"));
    }
}
