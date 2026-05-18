use rkyv::access;
use rkyv::rancor::Error;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Value, async_with};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use sugg::cache::{CommandNode, CompletionCache, get_cache_path};
use sugg::js::runtime::inject_globals;
use sugg::log_error;

fn json_to_command_node(v: JsonValue) -> CommandNode {
    serde_json::from_value(v).unwrap_or_default()
}

struct EngineArgs {
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
    cache_dir: Option<PathBuf>,
}

/// 解析子命令后的参数（跳过 argv[0] 和 argv[1]）
fn parse_engine_args() -> EngineArgs {
    let mut completions_dir = None;
    let mut lang = None;
    let mut cache_dir = None;
    let mut parser = lexopt::Parser::from_args(std::env::args().skip(2));
    while let Ok(Some(arg)) = parser.next() {
        match arg {
            lexopt::Arg::Long("completions-dir") => {
                completions_dir = parser
                    .value()
                    .ok()
                    .map(|v| PathBuf::from(v.to_string_lossy().as_ref()));
            }
            lexopt::Arg::Long("lang") => {
                lang = parser
                    .value()
                    .ok()
                    .map(|v| v.to_string_lossy().into_owned());
            }
            lexopt::Arg::Long("cache-dir") => {
                cache_dir = parser
                    .value()
                    .ok()
                    .map(|v| PathBuf::from(v.to_string_lossy().as_ref()));
            }
            _ => {}
        }
    }
    EngineArgs {
        completions_dir,
        lang,
        cache_dir,
    }
}

async fn run_build(args: &EngineArgs) -> Result<(), Box<dyn std::error::Error>> {
    let dir_path = args
        .completions_dir
        .clone()
        .or_else(|| {
            std::env::var("SUGG_COMPLETIONS_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| sugg::default_completions_dir());
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
        println!(
            "⚠️ Completions directory not found. Auto-created at {}. Place TS/JS scripts in this directory and retry.",
            sugg::path_to_slash(&dir_path)
        );
        return Ok(());
    }
    println!(
        "📦 Scanning completion scripts directory: {}",
        sugg::path_to_slash(&dir_path)
    );

    let lang = args
        .lang
        .clone()
        .or_else(|| std::env::var("SUGG_LANG").ok())
        .unwrap_or_else(|| "en".to_string());
    let (bundled_static, dynamic_bundles) =
        sugg::build_bundles(&dir_path, &lang).await;

    // 脚本清单在 build_bundles() 内边扫描边打印，此处只处理空目录兜底
    if bundled_static.is_empty() {
        println!("⚠️ Completions directory is empty, no configuration was bundled.");
        return Ok(());
    }

    let mut cache = CompletionCache::default();
    let rt = AsyncRuntime::new().expect("❌ Failed to create QuickJS runtime");
    let ctx = AsyncContext::full(&rt)
        .await
        .expect("❌ Failed to create QuickJS context");

    async fn try_generate_root(
        ctx: Ctx<'_>,
        bundled_static: String,
    ) -> anyhow::Result<CommandNode> {
        use anyhow::Context;
        inject_globals(ctx.clone());
        let module_temp = rquickjs::Module::declare(ctx.clone(), "temp", bundled_static)
            .context("JS module declaration failed")?;
        let (eval_mod, eval_val) = module_temp
            .eval()
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("JS module evaluation failed: {e:?}"))?;
        if let Some(promise) = eval_val.as_promise() {
            promise
                .clone()
                .into_future::<Value>()
                .await
                .catch(&ctx)
                .map_err(|e| {
                    anyhow::anyhow!("JS module top-level await execution failed: {e:?}")
                })?;
        }
        let config: rquickjs::Object = eval_mod
            .get("default")
            .context("Failed to get default export")?;
        let parse_func: Function = eval_mod
            .get("__parseConfig")
            .context("Failed to get __parseConfig function")?;
        let result: rquickjs::Object = parse_func
            .call((config,))
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("__parseConfig execution failed: {e:?}"))?;
        let json_str: String = result
            .get::<_, rquickjs::Value>("root")
            .and_then(|v| {
                let j: rquickjs::Object = ctx.globals().get("JSON")?;
                let s: Function = j.get("stringify")?;
                s.call((v,))
            })
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("Failed to serialize root node: {e:?}"))?;
        serde_json::from_str::<JsonValue>(&json_str)
            .map(|v| json_to_command_node(v))
            .context("JSON deserialization to CommandNode failed")
    }

    let root_result =
        async_with!(ctx => |ctx| { try_generate_root(ctx, bundled_static).await }).await;

    match root_result {
        Ok(final_root) => {
            let mut seen = std::collections::HashSet::new();
            for cmd in &final_root.subcommands {
                if !seen.insert(&cmd.name) {
                    log_error!(
                        "Duplicate top-level command '{}'. Different completion scripts cannot define the same command name. Use import to combine them into one file.",
                        cmd.name
                    );
                    std::process::exit(1);
                }
            }
            cache.root = final_root;
            for (stem, bundle, func_ids) in dynamic_bundles {
                let stem_name = stem.clone();
                let bc_result = async_with!(ctx => |ctx| {
                    use anyhow::Context;
                    let module = rquickjs::Module::declare(ctx.clone(), stem_name, bundle).context("Dynamic module declaration failed")?;
                    module.write(rquickjs::WriteOptions::default()).context("Bytecode generation failed")
                }).await;
                match bc_result {
                    Ok(bc) => {
                        let bc_idx = cache.bytecodes.len() as u32;
                        cache.bytecodes.push(bc);
                        for func_id in func_ids {
                            cache.dyn_index.push((func_id, bc_idx));
                        }
                    }
                    Err(e) => {
                        log_error!("Failed to compile bytecode for {}: {:#}", stem, e);
                    }
                }
            }
            let cache_path = args
                .cache_dir
                .as_ref()
                .map(|d| d.join(".completion_cache.bin"))
                .unwrap_or_else(|| {
                    std::env::var("SUGG_CACHE_DIR")
                        .ok()
                        .map(|d| PathBuf::from(d).join(".completion_cache.bin"))
                        .unwrap_or_else(get_cache_path)
                });
            if let Some(parent) = cache_path.parent() {
                fs::create_dir_all(parent).expect("Failed to create cache directory");
            }
            let bytes = rkyv::to_bytes::<Error>(&cache).expect("Failed to serialize cache");
            fs::write(cache_path, bytes).expect("Failed to write cache file");
            println!("✅ Cache complete!");
        }
        Err(e) => {
            log_error!("Cache build phase failed: {:#}", e);
        }
    }
    Ok(())
}

/// 打开日志文件（如存在）或其所在目录
fn run_log() {
    let log_path = sugg::logger::get_log_path();
    if log_path.exists() {
        println!("📄 Opening log file: {}", sugg::path_to_slash(&log_path));
        open_with_system(&log_path);
    } else if let Some(parent) = log_path.parent() {
        if parent.exists() {
            println!(
                "📂 Log file not found, opening containing directory: {}",
                sugg::path_to_slash(parent)
            );
            open_with_system(parent);
        } else {
            eprintln!(
                "❌ Log directory does not exist: {}",
                sugg::path_to_slash(parent)
            );
            std::process::exit(1);
        }
    }
}

/// 跨平台调用系统默认程序打开文件/目录
fn open_with_system(path: &std::path::Path) {
    let path_str = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        let status = std::process::Command::new("cmd")
            .args(["/c", "start", "", &path_str])
            .status();
        if let Err(e) = status {
            eprintln!("❌ Failed to open: {}", e);
            std::process::exit(1);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("open").arg(&*path_str).status();
        if let Err(e) = status {
            eprintln!("❌ Failed to open: {}", e);
            std::process::exit(1);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::process::Command::new("xdg-open")
            .arg(&*path_str)
            .status();
        if let Err(e) = status {
            eprintln!("❌ Failed to open: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_i18n_gen(args: &EngineArgs) {
    let completions_dir = args
        .completions_dir
        .clone()
        .or_else(|| {
            std::env::var("SUGG_COMPLETIONS_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| sugg::default_completions_dir());
    if !completions_dir.exists() {
        fs::create_dir_all(&completions_dir).expect("Failed to create completions directory");
    }

    let preferred_lang = args
        .lang
        .clone()
        .or_else(|| std::env::var("SUGG_LANG").ok())
        .unwrap_or_else(|| "en".to_string());

    // keys_map: namespace -> key -> lang -> translation
    let mut keys_map: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>> =
        BTreeMap::new();

    for (ns, i18n_dir) in sugg::scan_i18n_dirs(&completions_dir) {
        let Ok(dir_entries) = fs::read_dir(&i18n_dir) else {
            continue;
        };
        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                let lang = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Ok(s) = fs::read_to_string(&path) {
                    if let Ok(map) =
                        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s)
                    {
                        for (k, v) in map {
                            let val_str = if let Some(s) = v.as_str() {
                                s.replace('\n', " ").replace("*/", "* /")
                            } else {
                                v.to_string()
                            };
                            keys_map
                                .entry(ns.clone())
                                .or_default()
                                .entry(k)
                                .or_default()
                                .insert(lang.clone(), val_str);
                        }
                    }
                }
            }
        }
    }

    let mut s = String::new();
    if keys_map.is_empty() {
        s.push_str("declare const i18n: { readonly [key: string]: any };\n");
    } else {
        s.push_str("declare const i18n: {\n");

        if let Some(root_keys) = keys_map.get("") {
            for (key, translations) in root_keys {
                s.push_str("  /**\n");
                if let Some(text) = translations.get(&preferred_lang) {
                    s.push_str(&format!("   * - 🚩 **{}**: {}\n", preferred_lang, text));
                }
                for (lang, text) in translations {
                    if lang == &preferred_lang {
                        continue;
                    }
                    s.push_str(&format!("   * - **{}**: {}\n", lang, text));
                }
                s.push_str("   */\n");
                s.push_str(&format!("  readonly {}: string;\n", key));
            }
        }

        for (ns, ns_keys) in &keys_map {
            if ns.is_empty() {
                continue;
            }
            s.push_str(&format!("  readonly {}: {{\n", ns));
            for (key, translations) in ns_keys {
                s.push_str("    /**\n");
                if let Some(text) = translations.get(&preferred_lang) {
                    s.push_str(&format!("     * - 🚩 **{}**: {}\n", preferred_lang, text));
                }
                for (lang, text) in translations {
                    if lang == &preferred_lang {
                        continue;
                    }
                    s.push_str(&format!("     * - **{}**: {}\n", lang, text));
                }
                s.push_str("    */\n");
                s.push_str(&format!("    readonly {}: string;\n", key));
            }
            s.push_str("  };\n");
        }

        s.push_str("  readonly [key: string]: any;\n");
        s.push_str("};\n");
    }

    let out_path = completions_dir.join("i18n.d.ts");
    fs::write(&out_path, &s).expect("Failed to write i18n.d.ts");
    println!(
        "✅ Generated {} with {} namespaces.",
        sugg::path_to_slash(&out_path),
        keys_map.len()
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_engine_args();
    match std::env::args().nth(1).as_deref() {
        Some("i18n-gen") => {
            run_i18n_gen(&args);
            Ok(())
        }
        Some("log") => {
            run_log();
            Ok(())
        }
        Some("commands") => {
            let cache_path = sugg::cache::get_cache_path();
            if let Ok(data) = std::fs::read(&cache_path) {
                if let Ok(archived) =
                    access::<sugg::cache::structs::ArchivedCompletionCache, Error>(&data)
                {
                    for cmd in archived.root.subcommands.iter() {
                        println!("{}", cmd.name);
                    }
                }
            }
            Ok(())
        }
        Some("reload") => run_build(&args).await,

        _ => {
            eprintln!(
                "❌ Unknown subcommand: {}. Available commands: i18n-gen, log, commands, reload",
                std::env::args().nth(1).unwrap_or_default()
            );
            std::process::exit(1);
        }
    }
}
