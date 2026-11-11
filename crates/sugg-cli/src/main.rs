// Import custom cache management, struct definitions, JS runtime injection and error logging macros
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
// Import external crates: mmap for memory mapping, rkyv for zero-copy deserialization, rquickjs for embedding the JS engine
use anyhow::Context as _;
use memmap2::Mmap;
use rkyv::access;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, FromJs, Function, Value};
use std::collections::HashMap;
use std::fs::File;

/// Completion context enum
/// Node: staying on a command node (subcommands or options can be completed)
/// OptionValue: staying on an option that needs a value (may run a dynamic JS function or return a static array of suggestions)
enum Context<'a> {
    Node(&'a ArchivedCommandNode),
    OptionValue(&'a ArchivedOptionItem),
}

/// Internal enum for parsed option values
/// Flag: boolean option (args_count=0), mapped to JS true
/// Values: value-taking option (args_count>=1), mapped to JS string[], appends when the same option repeats
#[derive(Debug, Clone)]
enum ParsedValue<'a> {
    Flag,
    Values(Vec<&'a str>),
}

/// Option value waiting state: tracks how many tokens have been consumed while waiting.
/// `cap` = the option's args_count (total capacity), `used` = tokens consumed so far.
/// Released automatically when `used == cap`; remaining tokens follow the normal path.
struct WaitingState<'a> {
    opt: &'a ArchivedOptionItem,
    cap: u32,
    used: u32,
}

/// Return value of parse_cli_state
/// ctx: completion context (node or option waiting state)
/// options: argument table safely parsed by the engine
///   boolean arguments: value is ParsedValue::Flag
///   value-taking arguments: value is ParsedValue::Values(vec); repeated options append to the same array
///   all aliases of the same option are collected, so script authors can check any key
/// positional_remaining: remaining positional argument tokens for the node (0 means "released")
/// positional_args: positional values already consumed by the current node (flat by order; cleared on subcommand switch)
///   injected into the dynamic function ctx.args so scripts can decide completions from "values already filled in"
struct ParseResult<'a> {
    ctx: Context<'a>,
    options: HashMap<&'a str, ParsedValue<'a>>,
    positional_remaining: u32,
    positional_args: Vec<String>,
}

/// Core state machine: zero-copy cursor
/// Walks the words the user typed to find the matching node in the completion tree, with no heap allocation
/// Also safely collects parsed options and their values, avoiding false positives from flat words.includes()
///
/// Unified model: every node (command/option) consumes N tokens, declared by args_count.
///  - boolean option (args_count=0): consumes no token
///  - single-value option (args_count=1): consumes 1 token
///  - multi-value option (args_count=N): consumes N tokens, released automatically when full
///  - repeat mode: each option label occurrence opens a new waiting state, Values(vec) accumulates
///  - unknown option (not in current.options): degrade, keep the cursor on current, leave remaining counts untouched
fn parse_cli_state<'a>(root: &'a ArchivedCommandNode, words: &[&'a str]) -> ParseResult<'a> {
    let mut current = root;
    let mut waiting: Option<WaitingState<'a>> = None;
    let mut parsed_options: HashMap<&'a str, ParsedValue<'a>> = HashMap::new();
    let mut positional_remaining: u32 = current.args_count.into();
    let mut positional_args: Vec<String> = Vec::new();
    let mut no_more_options = false;

    for word in words {
        // 0. Explicit options (labels) must be consumed before waiting: avoid repeated labels being taken as values
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
                // Terminate current waiting (a repeated option also ends the old one; repeats accumulate into Values(vec))
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

        // 1. waiting consumption: only when the token is not an option label
        if let Some(w) = waiting.as_mut() {
            if w.used < w.cap {
                // Capacity left: consume the current token
                for label in w.opt.labels.iter() {
                    let entry = parsed_options
                        .entry(label.as_str())
                        .or_insert_with(|| ParsedValue::Values(Vec::new()));
                    if let ParsedValue::Values(vec) = entry {
                        vec.push(word);
                    }
                }
                w.used += 1;
                // Exactly full after consuming: release at once so later tokens follow the normal path
                if w.used >= w.cap {
                    waiting = None;
                }
                continue;
            } else {
                // Capacity already full, release waiting; the current token goes down the normal path again
                waiting = None;
            }
        }

        // 2. Standard -- separator: all following words become positional args, options are no longer parsed
        if *word == "--" {
            no_more_options = true;
            continue;
        }

        // 4. Positional argument: consume one token, accumulate flat into positional_args
        // Unified rule: every node (command/option x dynamic/static) consumes strictly by args_count,
        // with no special cases such as "dynamic nodes are implicitly unlimited"
        if positional_remaining > 0 {
            positional_args.push(word.to_string());
            positional_remaining -= 1;
            continue;
        }

        // 5. Subcommand matching
        if let Ok(idx) = current
            .subcommands
            .binary_search_by(|c| c.name.as_str().cmp(word))
        {
            let matched = &current.subcommands[idx];
            // O(1) teleport: jump straight to the main command node via the target index
            current = match &matched.target {
                rkyv::option::ArchivedOption::Some(target_idx) => current
                    .subcommands
                    .get(target_idx.to_native() as usize)
                    .unwrap_or(matched),
                rkyv::option::ArchivedOption::None => matched,
            };
            // Terminate waiting when switching to a new node (a different context should not keep consuming)
            waiting = None;
            positional_remaining = current.args_count.into();
            // Reset the positional argument list across nodes (parent info is read from the words array)
            positional_args.clear();
            continue;
        }

        // 6. Over mode: positional_remaining=0 + unknown token, silently ignored
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

/// Get the engine path: use the global install root directly
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

    // Skip "sugg" itself
    let mut args_iter = std::env::args().skip(1).peekable();

    // Skip the "complete" subcommand (if present)
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
            // `-h` / `--help` are forwarded to the engine immediately. After `--`, lexopt yields all following
            // tokens as Arg::Value (no longer as flags), so `sugg complete -- -h`
            // is not mistaken for a help request
            lexopt::Arg::Short('h') | lexopt::Arg::Long("help") => {
                delegate_to_engine();
            }
            lexopt::Arg::Value(v) => {
                if shell.is_none() {
                    // The first positional argument is the Shell name
                    let shell_str = v.to_string_lossy().into_owned();
                    shell = Some(shell_str.parse::<Shell>().unwrap_or_else(|e| {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }));
                } else {
                    // Later positional arguments (including those after --) are completion input words
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

/// Forward to the engine: single exit point; cfg!() runtime checks fully eliminate cfg blocks
fn delegate_to_engine() -> ! {
    let path = engine_path();
    let args: Vec<String> = std::env::args().skip(1).collect();

    let spawn_res = (|| -> std::io::Result<i32> {
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("sugg-engine not found: {}", path.display()),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(std::process::Command::new(&path).args(&args).exec())
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
            eprintln!(
                "{} failed to start sugg-engine: {}",
                sugg_core::ICON_ERROR,
                e
            );
            std::process::exit(1);
        }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // All non-complete commands are forwarded to sugg-engine; complete enables UI log interception
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

    // Extract the "prefix" the user is currently typing (the last word) and the preceding context words
    let mut prefix = String::new();

    if !words.is_empty() {
        let last = words.pop().unwrap();
        words.retain(|w| !w.is_empty());

        // Treat it directly as a normal prefix; do not split -opt= syntax
        // because different Shells handle '=' very differently, and forcing a split easily causes double-equals bugs
        prefix = last.to_string();
    }
    let words_before_prefix: &[&str] = &words;

    // Open the completion cache file and map it into memory with mmap
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
    // Use rkyv zero-copy access to the data structure in the memory map
    let archived = match access::<ArchivedCompletionCache, rkyv::rancor::Error>(&mmap) {
        Ok(a) => a,
        Err(_) => {
            print_results(vec![], &parsed.shell);
            return;
        }
    };

    // Run the state machine to derive the completion position and get the safely parsed option table
    let parse_result = parse_cli_state(&archived.root, words_before_prefix);
    // Extract options up front so partial moves in match arms do not make them unusable in multiple branches
    let parsed_options = parse_result.options;
    // Positional values already consumed by the current node (for dynamic ctx.args)
    let positional_args = parse_result.positional_args;
    // "Positional argument channel open" is equivalent to the node still having positional quota left
    let is_positional_mode = parse_result.positional_remaining > 0;

    // Refine the completion context: complete options or subcommands+args depending on whether the prefix starts with '-'
    enum EffectiveCtx<'a> {
        Options(&'a ArchivedCommandNode),
        SubcommandsAndArgs(&'a ArchivedCommandNode),
        OptionValue(&'a ArchivedOptionItem),
    }

    let mut opt_eq_prefix: Option<String> = None;
    let mut resolve_prefix = prefix.clone();

    let effective_ctx = match parse_result.ctx {
        Context::Node(node) if prefix.starts_with('-') => {
            if let Some(sep) = prefix.find('=') {
                let opt_name = prefix[..sep].to_string();
                let opt_val = prefix[sep + 1..].to_string();
                let has_label_starts_with = node.options.iter().any(|o| {
                    o.labels
                        .iter()
                        .any(|l| l.as_str().starts_with(prefix.as_str()))
                });
                if has_label_starts_with {
                    EffectiveCtx::Options(node)
                } else if let Some(opt) = node
                    .options
                    .iter()
                    .find(|o| o.labels.iter().any(|l| l.as_str() == opt_name.as_str()))
                {
                    opt_eq_prefix = Some(format!("{}=", opt_name));
                    resolve_prefix = opt_val;
                    EffectiveCtx::OptionValue(opt)
                } else {
                    EffectiveCtx::Options(node)
                }
            } else {
                EffectiveCtx::Options(node)
            }
        }
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
            items.extend(node.subcommands.iter().map(|c| {
                let style = c.style.as_ref().map(from_archived_style);
                CompletionItem::new(c.name.to_string(), c.description.to_string(), style)
                    .with_trailing_space()
            }));
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
                    &resolve_prefix,
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

    if let Some(eq_prefix) = &opt_eq_prefix {
        for item in &mut items {
            item.value = format!("{}{}", eq_prefix, item.value);
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

    // Zsh: logs go through the __msg__ protocol, not mixed into the completion menu
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

    // Put the first item on top (so a normal command is selected by default), insert logs after it, append the rest
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
                // Fish discards display, so icon and log level are folded into value, keeping the # prefix for safety
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

    // Only add a shim when there are no valid items, so a single log entry is not auto-inserted
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

/// Extracted shared helper: parses static arguments or runs JS to get dynamic completion items
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

/// Run embedded JavaScript to dynamically get completion items
/// Suited to completions that need real-time logic (e.g. reading directories, querying databases)
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
    /// Execute dynamic completion in a JS context (inner try function returning Result)
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
        // Inject global APIs (fetch, fs, etc., depending on inject_globals implementation)
        inject_globals(ctx.clone());

        // Load the precompiled binary bytecode (far faster than parsing text)
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

        // Build the context object passed to the JS function
        let ctx_obj = rquickjs::Object::new(ctx.clone()).unwrap();
        let _ = ctx_obj.set("prefix", prefix);
        let _ = ctx_obj.set("path", path);
        let _ = ctx_obj.set("words", words);
        // Positional values already consumed by the current node (accumulated flat; reset on subcommand switch)
        let _ = ctx_obj.set("positionals", positional_args);

        // Inject the option table safely parsed by the engine
        // boolean arguments: value is true (a JS boolean)
        // value-taking arguments: always a string[] array (one occurrence is a one-element array, repeats hold all values)
        // all aliases of the same option appear in this object
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

        // Call the JS function and handle a possible Promise return value
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
                .map_err(|e| match e {
                    rquickjs::CaughtError::Exception(ex) => {
                        let msg = ex.message().unwrap_or_default();
                        if msg.is_empty() {
                            anyhow::anyhow!("JS exception: {ex:?}")
                        } else {
                            anyhow::anyhow!("{msg}")
                        }
                    }
                    rquickjs::CaughtError::Value(v) => {
                        anyhow::anyhow!("Promise rejected with non-Error value: {v:?}")
                    }
                    rquickjs::CaughtError::Error(err) => {
                        anyhow::anyhow!("{err:?}")
                    }
                })?
        } else {
            js_result
        };

        // Silently handle null/undefined: a JS dynamic explicitly "not completing" equals "returning an empty array",
        // so no ERR log is written (avoids polluting the completion menu). Only true/false/numbers and other non-array values log ERR.
        if resolved.is_null() || resolved.is_undefined() {
            return Ok(Vec::new());
        }

        // Try parsing as {value, description, aliases?}[] first
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
                    // Expand aliases
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
                // Fallback: try parsing as string[]
                let as_strings: Result<Vec<String>, _> =
                    FromJs::from_js(&ctx, resolved).catch(&ctx);
                match as_strings {
                    Ok(strs) => {
                        for s in strs {
                            results.push(
                                CompletionItem::simple(s, String::new()).with_trailing_space(),
                            );
                        }
                    }
                    Err(e) => log_error!("JS return value parse error: {:?}", e),
                }
            }
        }
        Ok(results)
    }

    // Create the QuickJS async runtime
    let rt = AsyncRuntime::new().expect("Failed to create QuickJS Runtime");
    let ctx = AsyncContext::full(&rt)
        .await
        .expect("Failed to create QuickJS Context");

    // Execute inside the context, catching errors via match
    ctx.async_with(async |ctx| {
        match try_execute(
            ctx,
            func_name,
            bytecode,
            prefix,
            path,
            words,
            options,
            shell,
            positional_args,
        )
        .await
        {
            Ok(items) => items,
            Err(e) => {
                log_error!("{:#}", e);
                Vec::new()
            }
        }
    })
    .await
}
