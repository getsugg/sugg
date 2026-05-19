// 导入自定义的缓存管理、结构体定义、JS 运行时注入及错误处理宏
use sugg::Shell;
use sugg::cache::get_cache_path;
use sugg::cache::print_results;
use sugg::cache::{
    CompletionItem, SuggestionStyle,
    structs::{
        ArchivedCommandNode, ArchivedCompletionCache, ArchivedOptionItem, ArchivedSuggestionStyle,
    },
};
use sugg::js::runtime::inject_globals;
use sugg::log_error;
// 导入外部库：mmap 用于内存映射，rkyv 用于零拷贝反序列化，rquickjs 用于嵌入 JS 引擎
use anyhow::Context as _;
use memmap2::Mmap;
use rkyv::access;
use rquickjs::{
    AsyncContext, AsyncRuntime, CatchResultExt, Ctx, FromJs, Function, Value, async_with,
};
use std::collections::HashMap;
use std::fs::File;

/// 补全上下文枚举
/// Node: 停留在一个命令节点（可以补全子命令或选项）
/// OptionValue: 停留在需要参数值的选项上（可能需要运行动态 JS 函数或返回静态数组来获取参数建议）
enum Context<'a> {
    Node(&'a ArchivedCommandNode),
    OptionValue(&'a ArchivedOptionItem),
}

/// 解析出的选项值的内部枚举
/// Flag: 布尔选项（takes_value=false），对应 JS true
/// Values: 传值选项（takes_value=true），对应 JS string[]，支持同一选项多次出现时追加
#[derive(Debug, Clone)]
enum ParsedValue<'a> {
    Flag,
    Values(Vec<&'a str>),
}

/// parse_cli_state 的返回结果
/// ctx: 补全上下文（节点或选项等待状态）
/// options: 引擎安全解析出的参数表
///   布尔型参数：值为 ParsedValue::Flag
///   传值型参数：值为 ParsedValue::Values(vec)，同一选项多次出现时追加到同一数组
///   同一选项的所有别名都会被收集，脚本作者可检查任意一个 key
struct ParseResult<'a> {
    ctx: Context<'a>,
    options: HashMap<&'a str, ParsedValue<'a>>,
    is_positional_mode: bool,
}

/// 核心状态机：零拷贝游标
/// 功能：遍历用户输入的单词，在补全树中寻找匹配的节点，且全程不产生堆分配
/// 同时安全收集已解析的选项及其值，避免扁平的 words.includes() 误判
fn parse_cli_state<'a>(root: &'a ArchivedCommandNode, words: &[&'a str]) -> ParseResult<'a> {
    let mut current = root;
    let mut waiting: Option<&'a ArchivedOptionItem> = None;
    let mut parsed_options: HashMap<&'a str, ParsedValue<'a>> = HashMap::new();
    let mut is_positional_mode = false;
    // 新增：标记是否已遇到 '--'，之后不再解析选项，但允许子命令
    let mut no_more_options = false;

    for word in words {
        // 如果上一个单词是一个需要值的 Option（如 --file），则消耗当前单词作为其值
        if let Some(opt) = waiting.take() {
            // 将该选项的所有别名都关联到这个值，支持同一选项多次出现时追加
            for label in opt.labels.iter() {
                let entry = parsed_options
                    .entry(label.as_str())
                    .or_insert_with(|| ParsedValue::Values(Vec::new()));
                if let ParsedValue::Values(vec) = entry {
                    vec.push(word);
                }
            }
            continue;
        }

        // 标准的 -- 分隔符：之后所有单词强制为位置参数，不再解析选项
        if *word == "--" {
            no_more_options = true;
            continue;
        }

        // 位置参数模式下，跳过所有选项/子命令解析
        if is_positional_mode {
            continue;
        }

        if !no_more_options && word.starts_with('-') {
            // 处理选项：查找 labels 中是否包含当前输入的 word
            if let Some(opt) = current
                .options
                .iter()
                .find(|o| o.labels.iter().any(|l| l == word))
            {
                if opt.takes_value {
                    // 该选项需要后续参数，进入等待状态
                    waiting = Some(opt);
                } else {
                    // 布尔选项：标记为 Flag（多次出现依然是 Flag）
                    for label in opt.labels.iter() {
                        parsed_options.insert(label.as_str(), ParsedValue::Flag);
                    }
                }
            }
        } else {
            // 处理子命令：仅在没有遇到未知位置参数时才继续尝试匹配
            if !is_positional_mode {
                if let Ok(idx) = current
                    .subcommands
                    .binary_search_by(|c| c.name.as_str().cmp(word))
                {
                    let matched = &current.subcommands[idx];
                    // O(1) 瞬移：通过 target 下标直接跳转到主命令节点
                    current = match &matched.target {
                        rkyv::option::ArchivedOption::Some(target_idx) => current
                            .subcommands
                            .get(target_idx.to_native() as usize)
                            .unwrap_or(matched),
                        rkyv::option::ArchivedOption::None => matched,
                    };
                } else {
                    is_positional_mode = true;
                }
            }
        }
    }

    ParseResult {
        ctx: match waiting {
            Some(opt) => Context::OptionValue(opt),
            None => Context::Node(current),
        },
        options: parsed_options,
        is_positional_mode,
    }
}

/// 获取引擎路径：消除 4 处平台宏重复，用 cfg!() 一次判断
fn engine_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SUGG_ENGINE_PATH") {
        return std::path::PathBuf::from(p);
    }
    let exe_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };
    let base_dir = if cfg!(debug_assertions) {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default()
    } else {
        sugg::sugg_root()
    };
    base_dir.join(exe_name)
}

struct CompleteArgs {
    shell: Shell,
    cache_dir: Option<std::path::PathBuf>,
    input_words: Vec<String>,
    max_results: usize,
}

fn parse_complete_args() -> CompleteArgs {
    let mut shell: Option<Shell> = None;
    let mut cache_dir = None;
    let mut input_words = Vec::new();
    let mut max_results = 50;

    // 跳过 "sugg" 自身
    let mut args_iter = std::env::args().skip(1).peekable();

    // 跳过 "complete" 子命令（如果存在）
    if args_iter.peek().map(|s| s.as_str()) == Some("complete") {
        args_iter.next();
    }

    let mut parser = lexopt::Parser::from_args(args_iter);
    while let Ok(Some(arg)) = parser.next() {
        match arg {
            lexopt::Arg::Long("cache-dir") => {
                cache_dir = parser
                    .value()
                    .ok()
                    .map(|v| std::path::PathBuf::from(v.to_string_lossy().as_ref()));
            }
            lexopt::Arg::Long("max-results") => {
                max_results = parser
                    .value()
                    .ok()
                    .and_then(|v| v.to_string_lossy().parse().ok())
                    .unwrap_or(max_results);
            }
            lexopt::Arg::Value(v) => {
                if shell.is_none() {
                    // 第一个位置参数是 Shell 名称
                    let shell_str = v.to_string_lossy().into_owned();
                    shell = Some(shell_str.parse::<Shell>().unwrap_or_else(|e| {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }));
                } else {
                    // 后续的位置参数（包括 -- 后面的）都是补全输入词
                    input_words.push(v.to_string_lossy().into_owned());
                }
            }
            _ => {}
        }
    }

    CompleteArgs {
        shell: shell.unwrap_or_else(|| {
            eprintln!("Error: Missing <shell> argument. Usage: sugg complete <shell> -- <input>");
            std::process::exit(1);
        }),
        cache_dir,
        input_words,
        max_results,
    }
}

/// 转发给引擎：统一退出点，cfg!() 运行时判断彻底消除 cfg 块
fn delegate_to_engine() -> ! {
    let path = engine_path();
    let args: Vec<String> = std::env::args().skip(1).collect();

    let spawn_res = (|| -> std::io::Result<i32> {
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("未找到 sugg-engine: {}", path.display()),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            return Err(std::process::Command::new(&path).args(&args).exec());
        }
        #[cfg(not(unix))]
        {
            let status = std::process::Command::new(&path).args(&args).status()?;
            Ok(status.code().unwrap_or(1))
        }
    })();

    match spawn_res {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("❌ 启动引擎失败: {}", e);
            std::process::exit(1);
        }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // 非 complete 命令一律转发给 sugg-engine；complete 命令开启 UI 日志拦截
    if std::env::args().nth(1).as_deref() != Some("complete") {
        delegate_to_engine();
    } else {
        sugg::logger::set_ui_mode();
    }

    let parsed = parse_complete_args();

    // 处理补全逻辑
    let input = parsed.input_words.join(" ");

    // 单词切分与清理
    let mut words: Vec<&str> = if input.is_empty() {
        Vec::new()
    } else {
        input.split(' ').collect()
    };

    let clean_root_cmd;
    if let Some(first) = words.first_mut() {
        clean_root_cmd = std::path::Path::new(*first)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        *first = &clean_root_cmd;
    }

    // 提取用户正在输入的"前缀"（最后一个单词），以及之前的上下文单词
    if !words.is_empty() {
        let last = words.pop().unwrap();
        words.retain(|w| !w.is_empty());
        words.push(last);
    }

    let prefix = words.last().copied().unwrap_or("").to_string();
    let words_before_prefix: &[&str] = if words.len() > 1 {
        &words[..words.len() - 1]
    } else {
        &[]
    };

    // 打开补全缓存文件，使用 mmap 映射到内存
    let cache_path = parsed
        .cache_dir
        .as_ref()
        .map(|d| d.join(".completion_cache.bin"))
        .unwrap_or_else(get_cache_path);
    let file = match File::open(&cache_path) {
        Ok(f) => f,
        Err(_) => {
            sugg::cache::print_results(vec![], &parsed.shell);
            return;
        }
    };
    let mmap = match unsafe { Mmap::map(&file) } {
        Ok(m) => m,
        Err(_) => {
            sugg::cache::print_results(vec![], &parsed.shell);
            return;
        }
    };
    // 使用 rkyv 零拷贝访问内存映射中的数据结构
    let archived = match access::<ArchivedCompletionCache, rkyv::rancor::Error>(&mmap) {
        Ok(a) => a,
        Err(_) => {
            sugg::cache::print_results(vec![], &parsed.shell);
            return;
        }
    };

    // 运行状态机推导出补全位置，同时获取安全解析的选项表
    let parse_result = parse_cli_state(&archived.root, words_before_prefix);
    // 提取 options 避免 match 部分 move 导致无法在多个分支中使用
    let parsed_options = parse_result.options;
    let is_positional_mode = parse_result.is_positional_mode;

    // 细化补全上下文：根据前缀是否以 '-' 开头决定补全选项还是子命令+参数
    enum EffectiveCtx<'a> {
        Options(&'a ArchivedCommandNode),
        SubcommandsAndArgs(&'a ArchivedCommandNode),
        OptionValue(&'a ArchivedOptionItem),
    }

    let effective_ctx = match parse_result.ctx {
        Context::Node(node) if prefix.starts_with('-') => EffectiveCtx::Options(node),
        Context::Node(node) => EffectiveCtx::SubcommandsAndArgs(node),
        Context::OptionValue(opt) => EffectiveCtx::OptionValue(opt),
    };

    let mut items = Vec::new();

    match effective_ctx {
        EffectiveCtx::Options(node) => {
            items.extend(node.options.iter().flat_map(|o| {
                o.labels.iter().map(move |l| {
                    CompletionItem::new(
                        l.to_string(),
                        o.description.as_str().to_string(),
                        o.style.as_ref().map(from_archived_style),
                    )
                    .with_trailing_space()
                })
            }));
        }

        EffectiveCtx::SubcommandsAndArgs(node) => {
            // 子命令：如果遇到未命中的词汇，就不再补全下级命令
            if !is_positional_mode {
                items.extend(node.subcommands.iter().map(|c| {
                    let style = c.style.as_ref().map(from_archived_style);
                    CompletionItem::new(c.name.to_string(), c.description.to_string(), style)
                        .with_trailing_space()
                }));
            }

            // 动态/静态参数
            match (node.dynamic_func.as_deref(), node.static_args.as_ref()) {
                (Some(func_name), _) => {
                    if let Some(bytecode) = find_bytecode(archived, func_name) {
                        items.extend(
                            run_dynamic_js(
                                func_name,
                                &bytecode,
                                &prefix,
                                "",
                                words.clone(),
                                parsed_options.clone(),
                            )
                            .await,
                        );
                    }
                }
                (None, Some(static_args)) => {
                    items.extend(static_args.iter().map(|i| CompletionItem {
                        display: i.display.as_str().to_string(),
                        value: i.value.as_str().to_string(),
                        description: i.description.as_str().to_string(),
                        style: i.style.as_ref().map(from_archived_style),
                    }));
                }
                _ => {}
            }
        }

        EffectiveCtx::OptionValue(opt) => {
            match (opt.dynamic_func.as_deref(), opt.static_args.as_ref()) {
                (Some(func_name), _) => {
                    if let Some(bytecode) = find_bytecode(archived, func_name) {
                        items.extend(
                            run_dynamic_js(
                                func_name,
                                &bytecode,
                                &prefix,
                                "",
                                words.clone(),
                                parsed_options.clone(),
                            )
                            .await,
                        );
                    }
                }
                (None, Some(static_args)) => {
                    items.extend(static_args.iter().map(|i| CompletionItem {
                        display: i.display.as_str().to_string(),
                        value: i.value.as_str().to_string(),
                        description: i.description.as_str().to_string(),
                        style: i.style.as_ref().map(from_archived_style),
                    }));
                }
                _ => {}
            }
        }
    }

    handle_results(items, &prefix, &parsed.shell, parsed.max_results);
}

fn handle_results(items: Vec<CompletionItem>, prefix: &str, shell: &Shell, limit: usize) {
    let mut filtered: Vec<_> = items
        .into_iter()
        .filter(|i| i.value.starts_with(prefix) || i.display.starts_with(prefix))
        .take(limit)
        .collect();

    // 取出拦截到的 UI 日志（包括 Error, Warn，甚至 JS 脚本里的 log 调试信息）
    let logs = sugg::logger::get_ui_logs();
    if !logs.is_empty() {
        let mut ui_items = Vec::new();

        for (level, msg) in logs {
            ui_items.push(CompletionItem {
                // display 仅显示等级和图标，如 "❌ ERR" 或 "📝 LOG"
                display: format!("{} {}", level.icon(), level.text()),
                // value 和 description 承载完整的真实报错/调试信息
                value: msg.clone(),
                description: msg.clone(),
                style: Some(SuggestionStyle {
                    fg: Some(level.color().to_string()),
                    bg: None,
                    attr: Some(vec!["bold".to_string()]),
                }),
            });
        }

        // 垫片 Dummy 项：防止只有日志时被终端自动填入上屏
        ui_items.push(CompletionItem {
            display: " ".to_string(),
            value: format!("{} ", prefix),
            description: String::new(),
            style: None,
        });

        // 把日志项置顶显示
        ui_items.extend(filtered);
        filtered = ui_items;
    }

    print_results(filtered, shell);
}

fn from_archived_style(archived: &ArchivedSuggestionStyle) -> SuggestionStyle {
    SuggestionStyle {
        fg: archived.fg.as_ref().map(|s| s.as_str().to_string()),
        bg: archived.bg.as_ref().map(|s| s.as_str().to_string()),
        attr: archived
            .attr
            .as_ref()
            .map(|a| a.iter().map(|s| s.as_str().to_string()).collect()),
    }
}

fn find_bytecode(archived: &ArchivedCompletionCache, func_name: &str) -> Option<Vec<u8>> {
    let idx = archived
        .dyn_index
        .iter()
        .find(|t| t.0.as_str() == func_name)
        .map(|t| t.1.to_native() as usize)?;
    archived.bytecodes.get(idx).map(|bc| bc.as_slice().to_vec())
}

/// 运行嵌入式 JavaScript 动态获取补全项
/// 适合处理需要实时逻辑（如读取目录、查询数据库）的补全场景
async fn run_dynamic_js<'a>(
    func_name: &str,
    bytecode: &[u8],
    prefix: &str,
    path: &str,
    words: Vec<&'a str>,
    options: HashMap<&'a str, ParsedValue<'a>>,
) -> Vec<CompletionItem> {
    /// 在 JS 上下文中执行动态补全（内部 try 函数，返回 Result）
    async fn try_execute<'a>(
        ctx: Ctx<'_>,
        func_name: &str,
        bytecode: &[u8],
        prefix: &str,
        path: &str,
        words: Vec<&'a str>,
        options: HashMap<&'a str, ParsedValue<'a>>,
    ) -> anyhow::Result<Vec<CompletionItem>> {
        let mut results: Vec<CompletionItem> = Vec::new();
        // 注入全局 API（如 fetch, fs 等，取决于 inject_globals 的实现）
        inject_globals(ctx.clone());

        // 加载预编译好的二进制字节码（比解析文本快得多）
        let module = unsafe { rquickjs::Module::load(ctx.clone(), bytecode) }
            .context("JS module loading failed")?;
        let (eval_mod, _) = module
            .eval()
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("JS module evaluation failed: {e:?}"))?;
        let run_func: Function = eval_mod
            .get(func_name)
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("Failed to get startup function {}: {e:?}", func_name))?;

        // 构造传给 JS 函数的 context 对象
        let ctx_obj = rquickjs::Object::new(ctx.clone()).unwrap();
        let _ = ctx_obj.set("prefix", prefix);
        let _ = ctx_obj.set("path", path);
        let _ = ctx_obj.set("words", words);

        // 注入引擎安全解析的选项表
        // 布尔型参数：值为 true（JS 布尔值）
        // 传值型参数：永为 string[] 数组（出现一次是单元素数组，多次出现则包含所有值）
        // 同一选项的所有别名均会出现在此对象中
        let opts_obj = rquickjs::Object::new(ctx.clone()).unwrap();
        for (k, v) in options {
            match v {
                ParsedValue::Flag => {
                    let _ = opts_obj.set(k, true);
                }
                ParsedValue::Values(vec) => {
                    let _ = opts_obj.set(k, vec);
                }
            }
        }
        let _ = ctx_obj.set("options", opts_obj);

        // 调用 JS 函数并处理可能的 Promise 返回值
        let js_result: Value = run_func
            .call((ctx_obj,))
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("JS dynamic function call failed: {e:?}"))?;

        let resolved: Value = if let Some(promise) = js_result.as_promise() {
            promise
                .clone()
                .into_future::<Value>()
                .await
                .catch(&ctx)
                .map_err(|e| anyhow::anyhow!("Promise resolution failed: {e:?}"))?
        } else {
            js_result
        };

        // 优先尝试解析为 {value, description, aliases?}[]
        let as_objects: Result<Vec<rquickjs::Object>, _> =
            FromJs::from_js(&ctx, resolved.clone()).catch(&ctx);
        match as_objects {
            Ok(items) => {
                for obj in items {
                    let display: String = obj.get("display").unwrap_or_default();
                    let desc: String = obj.get("description").unwrap_or_default();
                    let style: Option<SuggestionStyle> = obj
                        .get::<_, rquickjs::Object>("style")
                        .ok()
                        .map(|s| SuggestionStyle {
                            fg: s.get("fg").ok(),
                            bg: s.get("bg").ok(),
                            attr: s.get("attr").ok(),
                        });
                    // 展开 aliases
                    if let Ok(aliases) = obj.get::<_, Vec<String>>("aliases") {
                        for alias in aliases {
                            results.push(
                                CompletionItem::new(alias, desc.clone(), style.clone())
                                    .with_trailing_space(),
                            );
                        }
                    }
                    let mut item = CompletionItem::new(display, desc, style);
                    item.value = if let Ok(explicit_value) = obj.get::<_, String>("value") {
                        explicit_value
                    } else {
                        format!("{} ", item.value)
                    };
                    results.push(item);
                }
            }
            Err(_) => {
                // 回退：尝试解析为 string[]
                let as_strings: Result<Vec<String>, _> =
                    FromJs::from_js(&ctx, resolved).catch(&ctx);
                match as_strings {
                    Ok(strs) => {
                        for s in strs {
                            results.push(CompletionItem::simple(s, String::new()));
                        }
                    }
                    Err(e) => log_error!("JS return value parse error: {:?}", e),
                }
            }
        }
        Ok(results)
    }

    // 创建 QuickJS 异步运行时
    let rt = AsyncRuntime::new().expect("Failed to create QuickJS Runtime");
    let ctx = AsyncContext::full(&rt)
        .await
        .expect("Failed to create QuickJS Context");

    // 在上下文中执行，通过 match 兜底错误
    async_with!(ctx => |ctx| {
        match try_execute(ctx, func_name, bytecode, prefix, path, words, options).await {
            Ok(items) => items,
            Err(e) => {
                log_error!("Dynamic JS execution failed: {:#}", e);
                Vec::new()
            }
        }
    })
    .await
}
