use oxc::allocator::Allocator;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::ast_visit::walk::{
    walk_call_expression, walk_object_expression, walk_object_property, walk_variable_declarator,
};
use oxc::parser::Parser;
use oxc::span::GetSpan;
use oxc::span::{SourceType, Span};
use std::collections::HashMap;

// 常量

pub const DYNAMIC_FUNC_PREFIX: &str = "__dyn_";
pub const IS_DYNAMIC_MARKER: &str = "__is_dynamic";
pub const DYNAMIC_ID_FIELD: &str = "id";

/// 已知的所有 sugg API 名称列表，用于 analyze_dynamic_apis
pub const SUGG_APIS: &[&str] = &[
    "exec", "execFile", "scanPath", "readFile", "readJson", "fetch", "cache", "ui",
];

// ─── 类型定义 ───

/// 动态调用的完整元信息
#[derive(Debug, Clone)]
pub struct DynamicInfo {
    pub full_span: Span,
    pub arg_span: Span,
    pub context_name: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize))]
pub struct ApiUsage {
    pub name: String,
    pub apis: Vec<String>,
}

// ─── AST 提取 ───

#[derive(Default)]
pub struct DynamicExtractor {
    pub dynamics: Vec<DynamicInfo>,
    pub create_completions: Vec<Span>,
    context_stack: Vec<String>,
}

impl<'a> Visit<'a> for DynamicExtractor {
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(ident) = &decl.id {
            self.context_stack.push(ident.name.to_string());
            walk_variable_declarator(self, decl);
            self.context_stack.pop();
        } else {
            walk_variable_declarator(self, decl);
        }
    }

    fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
        let key_name = match &prop.key {
            PropertyKey::StaticIdentifier(ident) => Some(ident.name.to_string()),
            PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
            _ => None,
        };
        if let Some(name) = key_name {
            self.context_stack.push(name);
            walk_object_property(self, prop);
            self.context_stack.pop();
        } else {
            walk_object_property(self, prop);
        }
    }

    fn visit_object_expression(&mut self, expr: &ObjectExpression<'a>) {
        let mut label_name = None;
        for prop in &expr.properties {
            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                let is_labels = match &p.key {
                    PropertyKey::StaticIdentifier(ident) => ident.name == "labels",
                    PropertyKey::StringLiteral(s) => s.value == "labels",
                    _ => false,
                };
                if is_labels && let Expression::ArrayExpression(arr) = &p.value {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::StringLiteral(s) = elem {
                            let clean = s
                                .value
                                .trim_start_matches('-')
                                .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
                            if !clean.is_empty() {
                                label_name = Some(clean);
                                break;
                            }
                        }
                    }
                }
            }
        }

        if let Some(name) = label_name {
            self.context_stack.push(name);
            walk_object_expression(self, expr);
            self.context_stack.pop();
        } else {
            walk_object_expression(self, expr);
        }
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        if let Expression::Identifier(ident) = &expr.callee {
            if ident.name == "dynamic" {
                if let Some(arg) = expr.arguments.first() {
                    let context_name = {
                        let filtered: Vec<String> = self
                            .context_stack
                            .iter()
                            .filter(|s| s.as_str() != "commands")
                            .map(|s| s.replace(|c: char| !c.is_ascii_alphanumeric(), "_"))
                            .collect();
                        if filtered.is_empty() {
                            "dynamic".to_string()
                        } else {
                            filtered.join("_")
                        }
                    };
                    self.dynamics.push(DynamicInfo {
                        full_span: expr.span,
                        arg_span: arg.span(),
                        context_name,
                    });
                }
            } else if ident.name == "createCompletion" {
                self.create_completions.push(expr.span);
            }
        }
        walk_call_expression(self, expr);
    }
}

struct RefCollector<'a> {
    scope: Span,
    refs: Vec<&'a str>,
}

impl<'a> Visit<'a> for RefCollector<'a> {
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        if self.scope.start <= ident.span.start && ident.span.end <= self.scope.end {
            self.refs.push(ident.name.as_str());
        }
    }
}

/// 生成最小动态模块：只保留 import 语句 + 动态函数依赖链所需的顶层声明 + 导出
pub fn generate_minimal_dynamic_module(
    source: &str,
    path: &str,
    dynamics: &[DynamicInfo],
    id_map: &HashMap<u32, String>,
) -> String {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let program = Parser::new(&allocator, source, source_type).parse().program;

    let mut name_to_idx: HashMap<&str, usize> = HashMap::new();
    let mut stmt_refs: Vec<Vec<&str>> = vec![vec![]; program.body.len()];

    for (i, stmt) in program.body.iter().enumerate() {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &d.id {
                        name_to_idx.insert(id.name.as_str(), i);
                    }
                }
            }
            Statement::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    name_to_idx.insert(id.name.as_str(), i);
                }
            }
            _ => {}
        }
        let mut collector = RefCollector {
            scope: stmt.span(),
            refs: vec![],
        };
        collector.visit_program(&program);
        stmt_refs[i] = collector.refs;
    }

    let mut needed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    for info in dynamics {
        let mut collector = RefCollector {
            scope: info.arg_span,
            refs: vec![],
        };
        collector.visit_program(&program);
        for name in collector.refs {
            if let Some(&idx) = name_to_idx.get(name)
                && needed.insert(idx)
            {
                queue.push_back(idx);
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        for &name in &stmt_refs[idx] {
            if let Some(&dep_idx) = name_to_idx.get(name)
                && needed.insert(dep_idx)
            {
                queue.push_back(dep_idx);
            }
        }
    }

    let mut out = String::new();
    for (i, stmt) in program.body.iter().enumerate() {
        let span = stmt.span();
        if matches!(stmt, Statement::ImportDeclaration(_)) || needed.contains(&i) {
            let text = &source[span.start as usize..span.end as usize];
            out.push_str(text);
            out.push('\n');
        }
    }
    out.push('\n');
    for info in dynamics {
        let id = &id_map[&info.full_span.start];
        let func_code = &source[info.arg_span.start as usize..info.arg_span.end as usize];
        out.push_str(&format!("export const {} = {};\n", id, func_code));
    }
    out
}

/// 提取源码中的 dynamic() 调用和 createCompletion 调用，返回 (modified_source, pure_dynamic_js, func_ids)
pub fn extract_dynamics(source: &str, path: &str) -> (String, String, Vec<String>) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut extractor = DynamicExtractor::default();
    extractor.visit_program(&parsed.program);

    let mut id_map: HashMap<u32, String> = HashMap::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();

    for info in &extractor.dynamics {
        let count = name_counts.entry(info.context_name.clone()).or_insert(0);
        let id = if *count == 0 {
            format!("{}{}", DYNAMIC_FUNC_PREFIX, info.context_name)
        } else {
            format!("{}{}_{}", DYNAMIC_FUNC_PREFIX, info.context_name, count)
        };
        *count += 1;
        id_map.insert(info.full_span.start, id);
    }

    let mut modified_source = source.to_string();
    let mut sorted_dynamics: Vec<&DynamicInfo> = extractor.dynamics.iter().collect();
    sorted_dynamics.sort_by_key(|b| std::cmp::Reverse(b.full_span.start));

    for info in &sorted_dynamics {
        let id = &id_map[&info.full_span.start];
        let replacement = format!(
            "{{ {}: true, {}: \"{}\" }}",
            IS_DYNAMIC_MARKER, DYNAMIC_ID_FIELD, id
        );
        modified_source.replace_range(
            info.full_span.start as usize..info.full_span.end as usize,
            &replacement,
        );
    }

    let pure_dynamic_js =
        generate_minimal_dynamic_module(source, path, &extractor.dynamics, &id_map);

    let func_ids: Vec<String> = extractor
        .dynamics
        .iter()
        .map(|info| id_map[&info.full_span.start].clone())
        .collect();

    (modified_source, pure_dynamic_js, func_ids)
}

// ─── API 审计 ───

pub fn analyze_dynamic_apis(dynamic_js: &str) -> Vec<ApiUsage> {
    struct ApiCollector {
        results: Vec<ApiUsage>,
        current_name: Option<String>,
        apis: Vec<String>,
    }

    impl<'a> Visit<'a> for ApiCollector {
        fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
            if let Some(decl) = &decl.declaration {
                if let Declaration::VariableDeclaration(var_decl) = decl {
                    for d in &var_decl.declarations {
                        if let BindingPattern::BindingIdentifier(ident) = &d.id {
                            let name = ident.name.to_string();
                            if name.starts_with(DYNAMIC_FUNC_PREFIX) {
                                self.current_name = Some(name);
                                self.apis.clear();
                                if let Some(init) = &d.init {
                                    self.visit_expression(init);
                                }
                                if let Some(current_name) = self.current_name.take() {
                                    if !self.apis.is_empty() || true {
                                        self.results.push(ApiUsage {
                                            name: current_name,
                                            apis: std::mem::take(&mut self.apis),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
            if self.current_name.is_some() {
                let name = ident.name.as_str();
                if SUGG_APIS.contains(&name) && !self.apis.contains(&name.to_string()) {
                    self.apis.push(name.to_string());
                }
            }
        }
    }

    if dynamic_js.trim().is_empty() {
        return vec![];
    }

    let allocator = Allocator::default();
    let source_type = SourceType::mjs();
    let parsed = Parser::new(&allocator, dynamic_js, source_type).parse();

    let mut collector = ApiCollector {
        results: vec![],
        current_name: None,
        apis: vec![],
    };
    collector.visit_program(&parsed.program);
    collector.results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dynamics() {
        let source = r#"
dynamic(async () => {
    return[{ value: "add", description: "Add project" }];
})
const addCommand = {
    args: dynamic(async () => {
        return[{ value: "add", description: "Add project" }];
    })
};

const config = createCompletion({
    options:[{ labels: ['-v'], description: 'version' }],
    args: dynamic(async () => {
        return [{ value: "run", description: "Run project" }];
    })
});

export default config;
        "#;

        let (modified_source, pure_dynamic_js, _func_ids) = extract_dynamics(source, "test.ts");

        assert!(modified_source.contains("__is_dynamic: true"));
        assert!(modified_source.contains("__dyn_dynamic"));
        assert!(modified_source.contains("__dyn_addCommand_args"));
        assert!(modified_source.contains("__dyn_config_args"));
        assert!(!modified_source.contains("dynamic(async () =>"));

        assert!(!pure_dynamic_js.contains("const config = null;"));
        assert!(!pure_dynamic_js.contains("dynamic(async () =>"));
        assert!(pure_dynamic_js.contains("export const __dyn_dynamic"));
        assert!(pure_dynamic_js.contains("export const __dyn_addCommand_args"));
        assert!(pure_dynamic_js.contains("export const __dyn_config_args"));
        assert!(
            pure_dynamic_js.contains(r#"return [{ value: "run", description: "Run project" }];"#)
        );
    }

    #[test]
    fn test_label_extraction() {
        let source = r#"
const myCmd = {
    options: [{
        labels: ['-v', '--verbose'],
        args: dynamic(async () => [{ value: "val", description: "a value" }])
    }]
};
export default myCmd;
        "#;

        let (modified_source, pure_dynamic_js, _func_ids) = extract_dynamics(source, "test.ts");

        assert!(modified_source.contains("__dyn_myCmd_options_v_args"));
        assert!(modified_source.contains("__is_dynamic: true"));
        assert!(pure_dynamic_js.contains("export const __dyn_myCmd_options_v_args"));
    }

    #[test]
    fn test_label_clean_symbolic() {
        let source = r#"
const cmd = {
    options: [{
        labels: ['--'],
        args: dynamic(async () => [])
    }]
};
export default cmd;
        "#;

        let (modified_source, _pure_dynamic_js, _func_ids) = extract_dynamics(source, "test.ts");

        assert!(modified_source.contains("__dyn_cmd_options_args"));
    }

    #[test]
    fn test_label_collision() {
        let source = r#"
const cmd = {
    options: [{
        labels: ['-v'],
        args: dynamic(async () => [{ value: "v1" }])
    }, {
        labels: ['-v'],
        args: dynamic(async () => [{ value: "v2" }])
    }]
};
export default cmd;
        "#;

        let (modified_source, pure_dynamic_js, _func_ids) = extract_dynamics(source, "test.ts");

        assert!(modified_source.contains("__dyn_cmd_options_v_args"));
        assert!(modified_source.contains("__dyn_cmd_options_v_args_1"));
        assert!(pure_dynamic_js.contains("export const __dyn_cmd_options_v_args"));
        assert!(pure_dynamic_js.contains("export const __dyn_cmd_options_v_args_1"));
    }

    #[test]
    fn test_analyze_apis() {
        let js = r#"
export const __dyn_git_add = async (ctx) => {
    const files = await exec("git status --porcelain", {});
    return files.split("\n").map(f => ({ value: f.trim() }));
};

export const __dyn_pnpm_run = async (ctx) => {
    const pkg = await readJson("package.json");
    return Object.keys(pkg.scripts).map(s => ({ value: s }));
};

export const __dyn_download = async (ctx) => {
    const res = await fetch("https://api.example.com/tags", {});
    const tags = await res.json();
    return tags.map(t => ({ value: t.name }));
};
        "#;

        let results = analyze_dynamic_apis(js);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].name, "__dyn_git_add");
        assert_eq!(results[0].apis, vec!["exec"]);
        assert_eq!(results[1].name, "__dyn_pnpm_run");
        assert_eq!(results[1].apis, vec!["readJson"]);
        assert_eq!(results[2].name, "__dyn_download");
        assert_eq!(results[2].apis, vec!["fetch"]);
    }

    #[test]
    fn test_analyze_apis_empty() {
        let results = analyze_dynamic_apis("");
        assert!(results.is_empty());

        let results = analyze_dynamic_apis("   ");
        assert!(results.is_empty());
    }

    #[test]
    fn test_analyze_apis_multiple_apis() {
        let js = r#"
export const __dyn_setup = async (ctx) => {
    const cfg = await readJson(".suggrc");
    const dir = cfg.dir || ".";
    const files = await scanPath(dir);
    await exec("mkdir -p " + dir, {});
    return files;
};
        "#;

        let results = analyze_dynamic_apis(js);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "__dyn_setup");
        let apis = &results[0].apis;
        assert!(apis.contains(&"readJson".to_string()));
        assert!(apis.contains(&"scanPath".to_string()));
        assert!(apis.contains(&"exec".to_string()));
    }
}
