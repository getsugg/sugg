/// 生成注入到每个用户脚本顶部的 import 语句。
pub fn generate_import_stmt() -> String {
    "import { createCompletion, readJson, __parseConfig } from 'virtual:env';\n".to_string()
}

/// 生成 virtual:env 模块内容（不含翻译，不含 globalThis）。
pub fn generate_env_code(lang: &str) -> String {
    let lang_json = serde_json::to_string(lang).unwrap();
    format!("const __LANG = {};\n{}", lang_json, include_str!("env.js"))
}

/// 生成 i18n 虚拟模块代码。
/// 每个命名空间生成独立子模块 `virtual:i18n/<ns>`（平铺 export），
/// 再生成一个聚合模块 `virtual:i18n`，通过 `export * as ns from '...'` 组合。
/// 用户代码通过 `import * as i18n from 'virtual:i18n'` → `i18n.ns.key` 访问。
pub fn generate_i18n_modules(
    translations_by_ns: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
) -> std::collections::HashMap<String, String> {
    use crate::bundler::VIRTUAL_I18N;

    let mut modules = std::collections::HashMap::new();
    let mut sorted_ns: Vec<&String> = translations_by_ns
        .keys()
        .filter(|k| !k.is_empty())
        .collect();
    sorted_ns.sort();

    // 每个命名空间生成独立子模块，平铺导出所有 key
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

    // 聚合模块：用 ES module re-export 语法组合所有命名空间
    let mut root_code = String::new();
    for ns in &sorted_ns {
        root_code.push_str(&format!("export * as {} from 'virtual:i18n/{}';\n", ns, ns));
    }
    modules.insert(VIRTUAL_I18N.to_string(), root_code);

    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_import_stmt() {
        let s = generate_import_stmt();
        assert!(s.contains("createCompletion"));
        assert!(s.contains("readJson"));
        assert!(s.contains("__parseConfig"));
        assert!(s.contains("from 'virtual:env'"));
        assert!(!s.contains("virtual:i18n"));
    }

    #[test]
    fn test_generate_env_code() {
        let code = generate_env_code("en");
        assert!(code.contains("const __LANG = \"en\""));
        assert!(code.contains("export const createCompletion"));
        assert!(code.contains("export const readJson"));
        assert!(code.contains("export const __parseConfig"));
        assert!(!code.contains("export const placeholder"));
        assert!(!code.contains("globalThis.__TRANSLATIONS"));
    }

    #[test]
    fn test_generate_i18n_modules() {
        use crate::bundler::VIRTUAL_I18N;

        let mut translations_by_ns = std::collections::HashMap::new();
        // 空命名空间（根）被跳过
        translations_by_ns.insert("".to_string(), std::collections::HashMap::new());

        let mut git_map = std::collections::HashMap::new();
        git_map.insert("commit".to_string(), "Commit".to_string());
        translations_by_ns.insert("git".to_string(), git_map);

        let mut docker_map = std::collections::HashMap::new();
        docker_map.insert("build".to_string(), "Build".to_string());
        translations_by_ns.insert("docker".to_string(), docker_map);

        let modules = generate_i18n_modules(&translations_by_ns);
        // 聚合模块
        assert!(modules.contains_key(VIRTUAL_I18N));
        let root_code = &modules[VIRTUAL_I18N];
        assert!(root_code.contains("export * as git from 'virtual:i18n/git'"));
        assert!(root_code.contains("export * as docker from 'virtual:i18n/docker'"));

        // 每个命名空间的独立子模块
        assert!(modules.contains_key("virtual:i18n/git"));
        assert!(modules["virtual:i18n/git"].contains("export const commit = \"Commit\";"));
        assert!(modules.contains_key("virtual:i18n/docker"));
        assert!(modules["virtual:i18n/docker"].contains("export const build = \"Build\";"));
    }
}
