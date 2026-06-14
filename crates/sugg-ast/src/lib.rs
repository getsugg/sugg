use oxc::allocator::Allocator;
use oxc::ast::ast::*;
use oxc::ast_visit::Visit;
use oxc::ast_visit::walk::{
    walk_call_expression, walk_object_expression, walk_object_property, walk_variable_declarator,
};
use oxc::parser::Parser;
use oxc::semantic::{ScopeFlags, Semantic, SemanticBuilder, SymbolId};
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

/// 通过 AST 剪枝收集顶层语句声明的所有 SymbolId
/// 在遇到函数/箭头函数/块级作用域时停止深入，保护局部变量不被收集
struct TopLevelBindingCollector {
    symbols: Vec<SymbolId>,
}

impl<'a> Visit<'a> for TopLevelBindingCollector {
    fn visit_binding_identifier(&mut self, ident: &BindingIdentifier<'a>) {
        if let Some(sym) = ident.symbol_id.get() {
            self.symbols.push(sym);
        }
    }

    fn visit_function(&mut self, func: &Function<'a>, _flags: ScopeFlags) {
        if let Some(id) = &func.id {
            self.visit_binding_identifier(id);
        }
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        if let Some(id) = &class.id {
            self.visit_binding_identifier(id);
        }
    }

    fn visit_arrow_function_expression(&mut self, _expr: &ArrowFunctionExpression<'a>) {}

    fn visit_block_statement(&mut self, _stmt: &BlockStatement<'a>) {}
}

struct RefCollector<'a, 'b> {
    scope: Span,
    referenced_symbols: std::collections::HashSet<SymbolId>,
    semantic: &'b Semantic<'a>,
}

impl<'a, 'b> RefCollector<'a, 'b> {
    fn new(scope: Span, semantic: &'b Semantic<'a>) -> Self {
        Self {
            scope,
            referenced_symbols: std::collections::HashSet::new(),
            semantic,
        }
    }
}

impl<'a, 'b> Visit<'a> for RefCollector<'a, 'b> {
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        if self.scope.start <= ident.span.start
            && ident.span.end <= self.scope.end
            && let Some(ref_id) = ident.reference_id.get()
        {
            let reference = self.semantic.scoping().get_reference(ref_id);
            if let Some(symbol_id) = reference.symbol_id() {
                self.referenced_symbols.insert(symbol_id);
            }
        }
    }
}

fn generate_minimal_dynamic_module_impl(
    source: &str,
    program: &Program<'_>,
    semantic: &Semantic<'_>,
    dynamics: &[DynamicInfo],
    id_map: &HashMap<u32, String>,
) -> String {
    let mut symbol_to_stmt_idx: HashMap<SymbolId, usize> = HashMap::new();

    // 1. 收集每个顶层语句声明的符号
    for (i, stmt) in program.body.iter().enumerate() {
        let mut collector = TopLevelBindingCollector {
            symbols: Vec::new(),
        };
        collector.visit_statement(stmt);
        for sym in collector.symbols {
            symbol_to_stmt_idx.insert(sym, i);
        }
    }

    let mut stmt_deps: Vec<Vec<usize>> = vec![vec![]; program.body.len()];
    for (i, stmt) in program.body.iter().enumerate() {
        let mut collector = RefCollector::new(stmt.span(), semantic);
        collector.visit_statement(stmt);
        for sym in collector.referenced_symbols {
            if let Some(&dep_idx) = symbol_to_stmt_idx.get(&sym)
                && dep_idx != i
            {
                stmt_deps[i].push(dep_idx);
            }
        }
    }

    let mut needed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    for info in dynamics {
        let mut collector = RefCollector::new(info.arg_span, semantic);
        collector.visit_program(program);
        for sym in collector.referenced_symbols {
            if let Some(&idx) = symbol_to_stmt_idx.get(&sym)
                && needed.insert(idx)
            {
                queue.push_back(idx);
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        for &dep_idx in &stmt_deps[idx] {
            if needed.insert(dep_idx) {
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
    let semantic = SemanticBuilder::new().build(&program).semantic;
    generate_minimal_dynamic_module_impl(source, &program, &semantic, dynamics, id_map)
}

/// 提取源码中的 dynamic() 调用和 createCompletion 调用，返回 (modified_source, pure_dynamic_js, func_ids)
pub fn extract_dynamics(source: &str, path: &str) -> (String, String, Vec<String>) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let program = &parsed.program;

    let mut extractor = DynamicExtractor::default();
    extractor.visit_program(program);

    let semantic = SemanticBuilder::new().build(program).semantic;

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

    let pure_dynamic_js = generate_minimal_dynamic_module_impl(
        source,
        program,
        &semantic,
        &extractor.dynamics,
        &id_map,
    );

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
            let Some(Declaration::VariableDeclaration(var_decl)) = &decl.declaration else {
                return;
            };

            for d in &var_decl.declarations {
                let BindingPattern::BindingIdentifier(ident) = &d.id else {
                    continue;
                };
                let name = ident.name.to_string();

                if !name.starts_with(DYNAMIC_FUNC_PREFIX) {
                    continue;
                }

                self.current_name = Some(name);
                self.apis.clear();

                if let Some(init) = &d.init {
                    self.visit_expression(init);
                }

                if let Some(current_name) = self.current_name.take() {
                    self.results.push(ApiUsage {
                        name: current_name,
                        apis: std::mem::take(&mut self.apis),
                    });
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

    // 验证变量遮蔽不会被误追踪为顶层引用
    // 用户在 dynamic 箭头函数体内 const [branches, files] = ...，
    // ...branches 中的 branches 是局部变量，不应把顶层 const branches = dynamic(getBranches) 拉进来
    #[test]
    fn test_shadowed_variable_not_included() {
        let source = r#"
async function getBranches() {
    return ["branch1", "branch2"];
}

const branches = dynamic(getBranches);

const cmd = {
    args: dynamic(async (ctx) => {
        const [branches, files] = await Promise.all([getBranches(), ["f1"]]);
        return [...branches, ...files];
    })
};

export default createCompletion({
    cmd: { description: "test", args: branches },
});
        "#;

        let (_, pure_dynamic_js, _) = extract_dynamics(source, "test.ts");

        // 顶层的 const branches = dynamic(getBranches) 不应该出现在动态 JS 中
        assert!(
            !pure_dynamic_js.contains("const branches = dynamic("),
            "shadowed branches declaration should not appear in dynamic JS"
        );
        // getBranches 函数应该存在（被 dynamic(getBranches) 引用）
        assert!(
            pure_dynamic_js.contains("function getBranches"),
            "getBranches function should be included"
        );
        // __dyn_branches 应该作为 export 存在
        assert!(
            pure_dynamic_js.contains("export const __dyn_branches = getBranches"),
            "__dyn_branches export should exist"
        );
    }

    #[test]
    fn test_shadowed_variable_in_pattern() {
        let source = r#"
async function getBranches() {
    return ["branch1", "branch2"];
}
async function getTags() {
    return ["tag1", "tag2"];
}

const branches = dynamic(getBranches);
const tags = dynamic(getTags);

const cmd = {
    args: dynamic(async (ctx) => {
        const [branches, tags] = await Promise.all([getBranches(), getTags()]);
        return [...branches, ...tags];
    })
};

export default createCompletion({
    cmd: { description: "test", args: [branches, tags] },
});
        "#;

        let (_, pure_dynamic_js, _) = extract_dynamics(source, "test.ts");

        assert!(
            !pure_dynamic_js.contains("const branches = dynamic("),
            "shadowed branches should not appear"
        );
        assert!(
            !pure_dynamic_js.contains("const tags = dynamic("),
            "shadowed tags should not appear"
        );
        assert!(pure_dynamic_js.contains("export const __dyn_branches = getBranches"));
        assert!(pure_dynamic_js.contains("export const __dyn_tags = getTags"));
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
