use rkyv::rancor::Error;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Value, async_with};
use serde_json::Value as JsonValue;
use std::fs;
use std::path::PathBuf;
use sugg_core::cache::{CommandNode, CompletionCache, get_cache_path};
use sugg_core::js::runtime::inject_globals;
use sugg_core::log_error;

fn json_to_command_node(v: JsonValue) -> CommandNode {
    serde_json::from_value(v).unwrap_or_default()
}

pub async fn run_build(
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
    cache_dir: Option<PathBuf>,
    debug_dump_dynamic: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir_path = completions_dir
        .clone()
        .or_else(|| {
            std::env::var("SUGG_COMPLETIONS_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(sugg_core::default_completions_dir);
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)?;
        println!(
            "{} Completions directory not found. Auto-created at {}. Place TS/JS scripts in this directory and retry.",
            sugg_core::ICON_INFO,
            sugg_core::path_to_slash(&dir_path)
        );
        return Ok(());
    }
    println!(
        "{} Scanning completion scripts directory: {}",
        sugg_core::ICON_PACKAGE,
        sugg_core::path_to_slash(&dir_path)
    );

    let lang = lang.clone().unwrap_or_else(crate::detect_locale);
    let (bundled_static, dynamic_bundles) = match crate::build_bundles(&dir_path, &lang).await {
        Ok(res) => res,
        Err(e) => {
            log_error!("Script Error: {:#}", e);
            return Ok(());
        }
    };

    if let Some(dump_dir) = &debug_dump_dynamic {
        fs::create_dir_all(dump_dir)?;
        for (stem, code, _) in &dynamic_bundles {
            let out_path = dump_dir.join(format!("{stem}.js"));
            fs::write(&out_path, code)?;
            println!(
                "{} Debug dump: {}",
                sugg_core::ICON_SCAN,
                out_path.display()
            );
        }
    }

    if bundled_static.is_empty() {
        println!(
            "{} Completions directory is empty, no configuration was bundled.",
            sugg_core::ICON_INFO
        );
        return Ok(());
    }

    let mut cache = CompletionCache::default();
    let rt = AsyncRuntime::new().expect("ERROR: Failed to create QuickJS runtime");
    let ctx = AsyncContext::full(&rt)
        .await
        .expect("ERROR: Failed to create QuickJS context");

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
            .map_err(|e| anyhow::anyhow!("JS module evaluation failed: {e}"))?;
        if let Some(promise) = eval_val.as_promise() {
            promise
                .clone()
                .into_future::<Value>()
                .await
                .catch(&ctx)
                .map_err(|e| anyhow::anyhow!("JS module top-level await execution failed: {e}"))?;
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
            .map_err(|e| anyhow::anyhow!("__parseConfig execution failed: {e}"))?;
        let json_str: String = result
            .get::<_, rquickjs::Value>("root")
            .and_then(|v| {
                let j: rquickjs::Object = ctx.globals().get("JSON")?;
                let s: Function = j.get("stringify")?;
                s.call((v,))
            })
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("Failed to serialize root node: {e}"))?;
        serde_json::from_str::<JsonValue>(&json_str)
            .map(json_to_command_node)
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
            // 排序 dyn_index 以支持二分查找
            cache.dyn_index.sort_by(|a, b| a.0.cmp(&b.0));

            let cache_path = cache_dir
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
            println!("{} Cache complete!", sugg_core::ICON_SUCCESS);
        }
        Err(e) => {
            log_error!("Cache build phase failed: {:#}", e);
        }
    }
    Ok(())
}
