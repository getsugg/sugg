// 导入自定义的缓存管理、结构体定义、JS 运行时注入及错误处理宏
use sugg_core::Shell;
use sugg_core::cache::get_cache_path;
use sugg_core::cache::print_results;
use sugg_core::cache::{
    CompletionItem, SuggestionStyle,
    structs::{
        ArchivedCommandNode, ArchivedCompletionCache, ArchivedOptionItem, ArchivedStaticSuggestion,
        ArchivedSuggestionStyle,
    },
};
use sugg_core::js::runtime::inject_globals;
use sugg_core::log_error;
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
/// Flag: 布尔选项（args_count=0），对应 JS true
/// Values: 传值选项（args_count>=1），对应 JS string[]，支持同一选项多次出现时追加
#[derive(Debug, Clone)]
enum ParsedValue<'a> {
    Flag,
    Values(Vec<&'a str>),
}

/// 选项值等待状态：跟踪 waiting 中已消耗的 token 数。
/// `cap` = 选项的 args_count（总容量），`used` = 已消费的 token 数。
/// `used == cap` 时自动释放，剩余 token 走正常路径。
struct WaitingState<'a> {
    opt: &'a ArchivedOptionItem,
    cap: u32,
    used: u32,
}

/// parse_cli_state 的返回结果
/// ctx: 补全上下文（节点或选项等待状态）
/// options: 引擎安全解析出的参数表
///   布尔型参数：值为 ParsedValue::Flag
///   传值型参数：值为 ParsedValue::Values(vec)，同一选项多次出现时追加到同一数组
///   同一选项的所有别名都会被收集，脚本作者可检查任意一个 key
/// positional_remaining: 节点剩余可消耗的位置参数 token 数（0 表示已"释放"）
/// positional_args: 当前节点已消耗的位置参数值（按消耗时间 flat 累计；切子命令时清空）
///   注入到 dynamic 函数的 ctx.args，供脚本按"之前已填的值"决定当前补全
struct ParseResult<'a> {
    ctx: Context<'a>,
    options: HashMap<&'a str, ParsedValue<'a>>,
    positional_remaining: u32,
    positional_args: Vec<String>,
}

/// 核心状态机：零拷贝游标
/// 功能：遍历用户输入的单词，在补全树中寻找匹配的节点，且全程不产生堆分配
/// 同时安全收集已解析的选项及其值，避免扁平的 words.includes() 误判
///
/// 统一模型：每个节点（command/option）都消耗 N 个 token，由 args_count 声明。
///  - boolean 选项（args_count=0）：不消耗 token
///  - 单值选项（args_count=1）：消耗 1 个 token
///  - 多值选项（args_count=N）：消耗 N 个 token，容量满自动释放
///  - 重复模式：每次选项 label 出现开新 waiting，Values(vec) 累加
///  - 未知选项（未在 current.options 中）：降级，光标留 current，不修改剩余计数
fn parse_cli_state<'a>(root: &'a ArchivedCommandNode, words: &[&'a str]) -> ParseResult<'a> {
    let mut current = root;
    let mut waiting: Option<WaitingState<'a>> = None;
    let mut parsed_options: HashMap<&'a str, ParsedValue<'a>> = HashMap::new();
    let mut positional_remaining: u32 = current.args_count.into();
    let mut positional_args: Vec<String> = Vec::new();
    let mut no_more_options = false;

    for word in words {
        // 0. 显式选项（标签）必须先于 waiting 消费：避免重复选项标签被误当成值
        if !no_more_options && word.starts_with('-') {
            let (opt_label, attached_value) = if word.contains('=') {
                let (lbl, val) = word.split_once('=').unwrap();
                (lbl, Some(val))
            } else {
                (*word, None)
            };

            if let Some(opt) = current
                .options
                .iter()
                .find(|o| o.labels.iter().any(|l| l == opt_label))
            {
                // 终止当前 waiting（同选项重复也终止旧的，重复行为由 Values(vec) 累加）
                waiting = None;
                let cap: u32 = opt.args_count.into();
                if cap == 0 {
                    for label in opt.labels.iter() {
                        parsed_options.insert(label.as_str(), ParsedValue::Flag);
                    }
                } else if let Some(val) = attached_value {
                    for label in opt.labels.iter() {
                        let entry = parsed_options
                            .entry(label.as_str())
                            .or_insert_with(|| ParsedValue::Values(Vec::new()));
                        if let ParsedValue::Values(vec) = entry {
                            vec.push(val);
                        }
                    }
                } else {
                    waiting = Some(WaitingState { opt, cap, used: 0 });
                }
                continue;
            }
        }

        // 1. waiting 消费：仅当 token 不是选项标签时
        if let Some(w) = waiting.as_mut() {
            if w.used < w.cap {
                // 还有容量：消费当前 token
                for label in w.opt.labels.iter() {
                    let entry = parsed_options
                        .entry(label.as_str())
                        .or_insert_with(|| ParsedValue::Values(Vec::new()));
                    if let ParsedValue::Values(vec) = entry {
                        vec.push(word);
                    }
                }
                w.used += 1;
                // 消费完正好满容量：立刻释放，让后续 token 走正常路径
                if w.used >= w.cap {
                    waiting = None;
                }
                continue;
            } else {
                // 容量已满，释放 waiting；当前 token 重新走正常路径
                waiting = None;
            }
        }

        // 2. 标准的 -- 分隔符：之后所有单词强制为位置参数，不再解析选项
        if *word == "--" {
            no_more_options = true;
            continue;
        }

        // 4. 位置参数：消耗一个 token，flat 累计进 positional_args
        // 统一规则：所有节点（command/option × dynamic/static）严格按 args_count 消耗，
        // 不做任何"dynamic 节点隐式无限"之类的特例
        if positional_remaining > 0 {
            positional_args.push(word.to_string());
            positional_remaining -= 1;
            continue;
        }

        // 5. 子命令匹配
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
            // 切到新节点时终止 waiting（不同上下文不应继续消费）
            waiting = None;
            positional_remaining = current.args_count.into();
            // 跨节点重置位置参数列表（父节点信息通过 words 数组取）
            positional_args.clear();
            continue;
        }

        // 6. 超模式：positional_remaining=0 + 未知 token，静默忽略
    }

    ParseResult {
        ctx: match waiting {
            Some(w) => Context::OptionValue(w.opt),
            None => Context::Node(current),
        },
        options: parsed_options,
        positional_remaining,
        positional_args,
    }
}

/// 获取引擎路径：直接使用全局安装根目录
fn engine_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("SUGG_ENGINE_PATH") {
        return std::path::PathBuf::from(p);
    }
    let exe_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };
    sugg_core::sugg_root().join(exe_name)
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
            // `-h` / `--help` 立即转发 engine。lexopt 在 `--` 之后会把后续 token
            // 全部作为 Arg::Value 产出（不再当 flag），所以 `sugg complete -- -h`
            // 不会被误判为 help 请求
            lexopt::Arg::Short('h') | lexopt::Arg::Long("help") => {
                delegate_to_engine();
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
            eprintln!("{} 启动引擎失败: {}", sugg_core::ICON_ERROR, e);
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
        sugg_core::logger::set_ui_mode();
    }

    let mut parsed = parse_complete_args();

    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    if let Some(first) = parsed.input_words.first_mut() {
        *first = std::path::Path::new(first.as_str())
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }

    let mut words: Vec<&str> = parsed.input_words.iter().map(|s| s.as_str()).collect();

    // 提取用户正在输入的"前缀"（最后一个单词），以及之前的上下文单词
    let mut prefix = String::new();

    if !words.is_empty() {
        let last = words.pop().unwrap();
        words.retain(|w| !w.is_empty());

        // 直接作为普通前缀对待，不拆分 -opt= 语法
        // 因为不同 Shell 对 '=' 的处理千差万别，强行拆分极易造成双等号 Bug
        prefix = last.to_string();
    }
    let words_before_prefix: &[&str] = &words;

    // 打开补全缓存文件，使用 mmap 映射到内存
    let cache_path = parsed
        .cache_dir
        .as_ref()
        .map(|d| d.join(".completion_cache.bin"))
        .unwrap_or_else(get_cache_path);
    let file = match File::open(&cache_path) {
        Ok(f) => f,
        Err(_) => {
            print_results(vec![], &parsed.shell);
            return;
        }
    };
    let mmap = match unsafe { Mmap::map(&file) } {
        Ok(m) => m,
        Err(_) => {
            print_results(vec![], &parsed.shell);
            return;
        }
    };
    // 使用 rkyv 零拷贝访问内存映射中的数据结构
    let archived = match access::<ArchivedCompletionCache, rkyv::rancor::Error>(&mmap) {
        Ok(a) => a,
        Err(_) => {
            print_results(vec![], &parsed.shell);
            return;
        }
    };

    // 运行状态机推导出补全位置，同时获取安全解析的选项表
    let parse_result = parse_cli_state(&archived.root, words_before_prefix);
    // 提取 options 避免 match 部分 move 导致无法在多个分支中使用
    let parsed_options = parse_result.options;
    // 当前节点已消耗的位置参数值（用于 dynamic ctx.args）
    let positional_args = parse_result.positional_args;
    // "位置参数通道开启"等价于节点还有可消耗的位置参数配额
    let is_positional_mode = parse_result.positional_remaining > 0;

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
            // 总是先列子命令（用户可能想切子命令上下文）
            items.extend(node.subcommands.iter().map(|c| {
                let style = c.style.as_ref().map(from_archived_style);
                CompletionItem::new(c.name.to_string(), c.description.to_string(), style)
                    .with_trailing_space()
            }));
            // 位置参数补全：仅在 is_positional_mode 时调用（remaining > 0 才有位置参数待补）
            // 所有节点统一规则，无 dynamic/static 特例
            if is_positional_mode {
                items.extend(
                    resolve_completions(
                        node.dynamic_func.as_deref(),
                        node.static_args.as_ref().map(|v| v.as_slice()),
                        archived,
                        &prefix,
                        &cwd,
                        &words,
                        &parsed_options,
                        &parsed.shell,
                        positional_args.clone(),
                    )
                    .await,
                );
            }
        }

        EffectiveCtx::OptionValue(opt) => {
            items.extend(
                resolve_completions(
                    opt.dynamic_func.as_deref(),
                    opt.static_args.as_ref().map(|v| v.as_slice()),
                    archived,
                    &prefix,
                    &cwd,
                    &words,
                    &parsed_options,
                    &parsed.shell,
                    positional_args.clone(),
                )
                .await,
            );
        }
    }

    handle_results(items, &prefix, &parsed.shell, parsed.max_results);
}

fn handle_results(items: Vec<CompletionItem>, prefix: &str, shell: &Shell, limit: usize) {
    let logs = sugg_core::logger::get_ui_logs();
    let filtered: Vec<_> = items
        .into_iter()
        .filter(|i| i.value.starts_with(prefix) || i.display.starts_with(prefix))
        .take(limit)
        .collect();

    // Zsh：日志走 __msg__ 协议，不混入补全菜单
    if *shell == Shell::Zsh {
        if !logs.is_empty() {
            for (level, msg) in &logs {
                println!("__msg__\t{} {}: {}", level.icon(), level.text(), msg);
            }
        }
        print_results(filtered, shell);
        return;
    }

    if logs.is_empty() {
        print_results(filtered, shell);
        return;
    }

    // 首项置顶（保证默认选中正常命令），日志插入其后，剩余补全项追加
    let mut final_items = Vec::new();
    let mut filtered_iter = filtered.into_iter();

    let has_valid = if let Some(first) = filtered_iter.next() {
        final_items.push(first);
        true
    } else {
        false
    };

    for (level, msg) in logs {
        let (display, description, value) = match shell {
            Shell::Powershell => (
                format!("{} {} {}", level.icon(), level.text(), msg),
                String::new(),
                format!("# {}", msg),
            ),
            Shell::Nushell => (
                format!("{} {}", level.icon(), level.text()),
                msg.clone(),
                format!("# {}", msg),
            ),
            Shell::Fish => {
                // Fish 丢弃了 display，将图标和日志级别拼入 value，保留 # 前缀以确保安全
                let val = format!("# {} {}: {}", level.icon(), level.text(), msg);
                (String::new(), String::new(), val)
            }
            _ => (
                format!("{} {}", level.icon(), level.text()),
                msg.clone(),
                format!("# {}", msg),
            ),
        };

        final_items.push(CompletionItem {
            display,
            value,
            description,
            style: Some(SuggestionStyle {
                fg: Some(level.color().to_string()),
                bg: None,
                attr: Some(vec!["bold".to_string()]),
            }),
        });
    }

    // 仅当无有效项时才加垫片，防止单条日志被自动填入
    if !has_valid {
        final_items.push(CompletionItem {
            display: " ".to_string(),
            value: format!("{} ", prefix),
            description: String::new(),
            style: None,
        });
    }

    final_items.extend(filtered_iter);
    print_results(final_items, shell);
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
        .binary_search_by(|t| t.0.as_str().cmp(func_name))
        .ok()
        .map(|i| archived.dyn_index[i].1.to_native() as usize)?;
    archived.bytecodes.get(idx).map(|bc| bc.as_slice().to_vec())
}

/// 提取出的公共辅助函数：用于解析静态参数或运行 JS 获取动态补全项
#[allow(clippy::too_many_arguments)]
async fn resolve_completions<'a>(
    dynamic_func: Option<&str>,
    static_args: Option<&[ArchivedStaticSuggestion]>,
    archived: &ArchivedCompletionCache,
    prefix: &str,
    path: &str,
    words: &[&str],
    parsed_options: &HashMap<&str, ParsedValue<'a>>,
    shell: &Shell,
    positional_args: Vec<String>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    match (dynamic_func, static_args) {
        (Some(func_name), _) => {
            if let Some(bytecode) = find_bytecode(archived, func_name) {
                items.extend(
                    run_dynamic_js(
                        func_name,
                        &bytecode,
                        prefix,
                        path,
                        words.to_vec(),
                        parsed_options.clone(),
                        shell,
                        positional_args,
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
    items
}

/// 运行嵌入式 JavaScript 动态获取补全项
/// 适合处理需要实时逻辑（如读取目录、查询数据库）的补全场景
#[allow(clippy::too_many_arguments)]
async fn run_dynamic_js<'a>(
    func_name: &str,
    bytecode: &[u8],
    prefix: &str,
    path: &str,
    words: Vec<&'a str>,
    options: HashMap<&'a str, ParsedValue<'a>>,
    shell: &Shell,
    positional_args: Vec<String>,
) -> Vec<CompletionItem> {
    /// 在 JS 上下文中执行动态补全（内部 try 函数，返回 Result）
    #[allow(clippy::too_many_arguments)]
    async fn try_execute<'a>(
        ctx: Ctx<'_>,
        func_name: &str,
        bytecode: &[u8],
        prefix: &str,
        path: &str,
        words: Vec<&'a str>,
        options: HashMap<&'a str, ParsedValue<'a>>,
        shell: &Shell,
        positional_args: Vec<String>,
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
        // 当前节点已消耗的位置参数值（flat 累计；切子命令时已重置）
        let _ = ctx_obj.set("positionals", positional_args);

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
        let _ = ctx_obj.set("shell", shell.as_str());
        let _ = ctx_obj.set("os", std::env::consts::OS);

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

        // 静默处理 null/undefined：JS dynamic 显式 "不补" 与 "返回空数组" 等价，
        // 不写 ERR 日志（避免污染补全菜单）。true/false/数字等其他非数组值才记 ERR。
        if resolved.is_null() || resolved.is_undefined() {
            return Ok(Vec::new());
        }

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
        match try_execute(ctx, func_name, bytecode, prefix, path, words, options, shell, positional_args).await {
            Ok(items) => items,
            Err(e) => {
                log_error!("Dynamic JS execution failed: {:#}", e);
                Vec::new()
            }
        }
    })
    .await
}
