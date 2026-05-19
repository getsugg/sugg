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

use crate::bundler::{DYNAMIC_FUNC_PREFIX, DYNAMIC_ID_FIELD, IS_DYNAMIC_MARKER};

/// 动态调用的完整元信息
#[derive(Debug, Clone)]
pub struct DynamicInfo {
    pub full_span: Span,
    pub arg_span: Span,
    /// 从 AST 父节点推断的上下文名称（变量名/属性名），无上下文时回退为 "dynamic"
    pub context_name: String,
}

#[derive(Default)]
pub struct DynamicExtractor {
    pub dynamics: Vec<DynamicInfo>,
    pub create_completions: Vec<Span>,
    /// 在 AST 遍历时维护的上下文栈，用于推断 dynamic() 的所在变量/属性名
    context_stack: Vec<String>,
}

impl<'a> Visit<'a> for DynamicExtractor {
    /// 追踪变量声明：访问声明器时将变量名入栈
    fn visit_variable_declarator(&mut self, decl: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(ident) = &decl.id {
            self.context_stack.push(ident.name.to_string());
            walk_variable_declarator(self, decl);
            self.context_stack.pop();
        } else {
            walk_variable_declarator(self, decl);
        }
    }

    /// 追踪对象属性：访问属性时将 key 名入栈
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

    /// 拦截对象字面量：如果对象包含 `labels` 属性（如 options 数组项），
    /// 提取第一个字符串字面量标签清洗后入栈，使 dynamic() 能生成语义化 ID。
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
                            // 清洗：去掉前导连字符，非字母数字替换为下划线
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
                    // 收集上下文路径，过滤掉无意义的 "commands" 键
                    let context_name = {
                        let filtered: Vec<&str> = self
                            .context_stack
                            .iter()
                            .filter(|s| s.as_str() != "commands")
                            .map(|s| s.as_str())
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

/// 提取源码中的 dynamic() 调用和 createCompletion 调用，返回 (modified_source, pure_dynamic_js)
///
/// - `source`: 原始 TypeScript/JavaScript 源码
/// - `path`: 文件路径（用于确定语言类型）
///
/// ID 格式: `__dyn_{context_path}`
/// 如同一上下文有多个 dynamic，自动追加序号: `__dyn_{context_path}_{n}`
pub fn extract_dynamics(source: &str, path: &str) -> (String, String, Vec<String>) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut extractor = DynamicExtractor::default();
    extractor.visit_program(&parsed.program);

    // 生成 ID 映射：基于 context_name 生成语义化 ID，碰撞时追加序号
    let mut id_map: HashMap<u32, String> = HashMap::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();

    for info in &extractor.dynamics {
        let count = name_counts.entry(info.context_name.clone()).or_insert(0);
        let id = if *count == 0 {
            format!("{}{}", DYNAMIC_FUNC_PREFIX, info.context_name)
        } else {
            format!("{}{}_{}",DYNAMIC_FUNC_PREFIX, info.context_name, count)
        };
        *count += 1;
        id_map.insert(info.full_span.start, id);
    }

    // 构建 modified_source（原位替换 dynamic(...) 为标记对象）
    let mut modified_source = source.to_string();
    let mut sorted_dynamics: Vec<&DynamicInfo> = extractor.dynamics.iter().collect();
    // 按起始位置降序，保证从后往前替换时偏移量不受影响
    sorted_dynamics.sort_by_key(|b| std::cmp::Reverse(b.full_span.start));

    for info in &sorted_dynamics {
        let id = &id_map[&info.full_span.start];
        let replacement = format!("{{ {}: true, {}: \"{}\" }}", IS_DYNAMIC_MARKER, DYNAMIC_ID_FIELD, id);
        modified_source.replace_range(
            info.full_span.start as usize..info.full_span.end as usize,
            &replacement,
        );
    }

    // --- 构建 pure_dynamic_js（基于 Span 精确抹除，避免全局字符串替换误伤） ---
    // 收集所有需要抹除的 span，过滤掉被其他更大 span 完全包含的（如 createCompletion 内的 dynamic）
    let mut raw_dyn: Vec<(Span, &str)> = Vec::new();
    for span in &extractor.create_completions {
        raw_dyn.push((*span, "null"));
    }
    for info in &extractor.dynamics {
        raw_dyn.push((info.full_span, "null"));
    }
    let mut dyn_replacements: Vec<(Span, &str)> = Vec::new();
    for &(span, text) in &raw_dyn {
        let is_contained = raw_dyn
            .iter()
            .any(|&(other, _)| other.start < span.start && span.end < other.end);
        if !is_contained {
            dyn_replacements.push((span, text));
        }
    }
    dyn_replacements.sort_by_key(|b| std::cmp::Reverse(b.0.start));

    let mut pure_dynamic_js = source.to_string();
    for (span, replacement) in &dyn_replacements {
        pure_dynamic_js.replace_range(span.start as usize..span.end as usize, replacement);
    }

    // 追加动态函数导出（按源码出现顺序）
    pure_dynamic_js.push_str("\n\n// --- Auto-appended dynamic exports by compiler ---\n");
    let mut func_ids: Vec<String> = Vec::new();
    for info in &extractor.dynamics {
        let id = &id_map[&info.full_span.start];
        let func_code = &source[info.arg_span.start as usize..info.arg_span.end as usize];
        pure_dynamic_js.push_str(&format!("export const {} = {};\n", id, func_code));
        func_ids.push(id.clone());
    }

    (modified_source, pure_dynamic_js, func_ids)
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

        // --- 1. 验证静态源码 (modified_source) ---
        assert!(modified_source.contains("__is_dynamic: true"));
        // 顶层 dynamic → fallback "dynamic" → __dyn_dynamic
        assert!(modified_source.contains("__dyn_dynamic"));
        // addCommand.args → 上下文路径 "addCommand_args" → __dyn_addCommand_args
        assert!(modified_source.contains("__dyn_addCommand_args"));
        // config 中 createCompletion 里的 args → 上下文路径 "config_args" → __dyn_config_args
        assert!(modified_source.contains("__dyn_config_args"));
        assert!(!modified_source.contains("dynamic(async () =>"));

        // --- 2. 验证纯动态 JS (pure_dynamic_js) ---
        assert!(pure_dynamic_js.contains("const config = null;"));
        assert!(pure_dynamic_js.contains("args: null"));
        assert!(!pure_dynamic_js.contains("dynamic(async () =>"));
        assert!(pure_dynamic_js.contains("// --- Auto-appended dynamic exports by compiler ---"));
        // 按源码出现顺序导出（语义化 ID，无 span.start 后缀）
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

        // labels['-v'] 提取出 "v"，上下文路径：myCmd → options → v → args
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

        // '--' 清洗后为空，不会入栈，上下文为 cmd → options → args
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

        // 两个都在 cmd → options → v → args 路径下，第二个碰撞，追加 _1
        assert!(modified_source.contains("__dyn_cmd_options_v_args"));
        assert!(modified_source.contains("__dyn_cmd_options_v_args_1"));
        assert!(pure_dynamic_js.contains("export const __dyn_cmd_options_v_args"));
        assert!(pure_dynamic_js.contains("export const __dyn_cmd_options_v_args_1"));
    }
}
