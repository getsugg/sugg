use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify};
use rkyv::rancor::Error;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Function, Value};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use sugg_core::cache::{CommandNode, CompletionCache, get_cache_path};
use sugg_core::js::runtime::inject_globals;
use sugg_core::log_error;

pub async fn run_build(
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
    cache_dir: Option<PathBuf>,
    debug_dump_dynamic: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_build_with_cache(
        completions_dir,
        lang,
        cache_dir,
        debug_dump_dynamic,
        &mut HashMap::new(),
    )
    .await
}

pub(crate) async fn run_build_with_cache(
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
    cache_dir: Option<PathBuf>,
    debug_dump_dynamic: Option<PathBuf>,
    file_cache: &mut HashMap<PathBuf, crate::CachedFile>,
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
    let t0 = std::time::Instant::now();
    let (static_results, dynamic_results) =
        match crate::build_bundles(&dir_path, &lang, file_cache).await {
            Ok(res) => res,
            Err(e) => {
                log_error!("Script Error: {:#}", e);
                return Ok(());
            }
        };
    println!("  ⏱ build_bundles took {:?}", t0.elapsed());

    let stem_cache: HashMap<String, PathBuf> = file_cache
        .iter()
        .map(|(p, f)| (f.stem.clone(), p.clone()))
        .collect();

    if let Some(dump_dir) = &debug_dump_dynamic {
        fs::create_dir_all(dump_dir)?;
        for (stem, code, _) in &dynamic_results {
            let out_path = dump_dir.join(format!("{stem}.js"));
            fs::write(&out_path, code)?;
            println!(
                "{} Debug dump: {}",
                sugg_core::ICON_SCAN,
                out_path.display()
            );
        }
    }

    if static_results.is_empty() {
        println!(
            "{} Completions directory is empty, no configuration was bundled.",
            sugg_core::ICON_INFO
        );
        return Ok(());
    }

    let t_runtime = std::time::Instant::now();
    let rt = AsyncRuntime::new().expect("ERROR: Failed to create QuickJS runtime");
    println!("  ⏱   AsyncRuntime::new took {:?}", t_runtime.elapsed());
    let t_ctx = std::time::Instant::now();
    let ctx = AsyncContext::full(&rt)
        .await
        .expect("ERROR: Failed to create QuickJS context");
    println!("  ⏱   AsyncContext::full took {:?}", t_ctx.elapsed());

    // ── 哪些文件需要 QuickJS 处理（跳过缓存命中的） ──
    let mut static_to_eval: Vec<(usize, String, String)> = Vec::new();
    let mut static_cached: Vec<(usize, CommandNode)> = Vec::new();

    for (idx, (stem, code)) in static_results.iter().enumerate() {
        if let Some(cached) = stem_cache.get(stem).and_then(|p| file_cache.get(p))
            && let Some(cn) = &cached.command_node
        {
            static_cached.push((idx, cn.clone()));
            continue;
        }
        static_to_eval.push((idx, stem.clone(), code.clone()));
    }

    let mut dyn_to_compile: Vec<(usize, String, String)> = Vec::new();
    let mut dyn_cached: Vec<(usize, Vec<u8>)> = Vec::new();

    for (idx, (stem, code, _)) in dynamic_results.iter().enumerate() {
        if let Some(cached) = stem_cache.get(stem).and_then(|p| file_cache.get(p))
            && let Some(bc) = &cached.bytecode
        {
            dyn_cached.push((idx, bc.clone()));
            continue;
        }
        dyn_to_compile.push((idx, stem.clone(), code.clone()));
    }

    // ── 批量 QuickJS：单文件静态评估 ──
    let t_eval = std::time::Instant::now();
    let mut fresh_nodes: Vec<(usize, CommandNode)> = Vec::new();
    if !static_to_eval.is_empty() {
        let eval_result: Vec<(usize, CommandNode)> = ctx
            .async_with(async |ctx| {
                use anyhow::Context;
                inject_globals(ctx.clone());
                let mut results = Vec::new();
                for (idx, stem, code) in &static_to_eval {
                    let module =
                        rquickjs::Module::declare(ctx.clone(), stem.as_str(), code.as_str())
                            .context("JS module declaration failed")?;
                    let (eval_mod, eval_val) = module
                        .eval()
                        .catch(&ctx)
                        .map_err(|e| anyhow::anyhow!("JS module evaluation failed: {e}"))?;
                    if let Some(promise) = eval_val.as_promise() {
                        promise
                            .clone()
                            .into_future::<Value>()
                            .await
                            .catch(&ctx)
                            .map_err(|e| {
                                anyhow::anyhow!("JS module top-level await failed: {e}")
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
                        .map_err(|e| anyhow::anyhow!("__parseConfig execution failed: {e}"))?;
                    let node: CommandNode = result
                        .get("root")
                        .context("Failed to get root from parseConfig result")?;
                    results.push((*idx, node));
                }
                Ok::<_, anyhow::Error>(results)
            })
            .await?;
        for (idx, node) in &eval_result {
            let stem = &static_results[*idx].0;
            if let Some(cached) = stem_cache.get(stem).and_then(|p| file_cache.get_mut(p)) {
                cached.command_node = Some(node.clone());
            }
            fresh_nodes.push((*idx, node.clone()));
        }
    }
    println!(
        "  ⏱   static eval ({} files) took {:?}",
        static_to_eval.len(),
        t_eval.elapsed()
    );

    // ── 批量 QuickJS：字节码编译 ──
    let t_bc = std::time::Instant::now();
    let mut fresh_bytecodes: Vec<(usize, Vec<u8>)> = Vec::new();
    for (idx, stem, code) in &dyn_to_compile {
        let bc_result = ctx
            .async_with(async |ctx| {
                use anyhow::Context;
                let module = rquickjs::Module::declare(ctx.clone(), stem.as_str(), code.as_str())
                    .context("Dynamic module declaration failed")?;
                module
                    .write(rquickjs::WriteOptions::default())
                    .context("Bytecode generation failed")
            })
            .await;
        match bc_result {
            Ok(bc) => {
                if let Some(cached) = stem_cache.get(stem).and_then(|p| file_cache.get_mut(p)) {
                    cached.bytecode = Some(bc.clone());
                }
                fresh_bytecodes.push((*idx, bc));
            }
            Err(e) => {
                return Err(
                    anyhow::anyhow!("Failed to compile bytecode for {}: {:#}", stem, e).into(),
                );
            }
        }
    }
    println!(
        "  ⏱   bytecode compile ({} files) took {:?}",
        dyn_to_compile.len(),
        t_bc.elapsed()
    );

    // ── 合并所有 CommandNode ──
    let mut all_nodes: Vec<(usize, CommandNode)> = static_cached;
    all_nodes.extend(fresh_nodes);
    all_nodes.sort_by_key(|(idx, _)| *idx);

    let mut root = CommandNode::default();

    let mut alias_mappings = Vec::new();
    for (_, node) in all_nodes {
        for sub in &node.subcommands {
            if let Some(local_target_idx) = sub.target
                && let Some(target_node) = node.subcommands.get(local_target_idx as usize)
            {
                alias_mappings.push((sub.name.clone(), target_node.name.clone()));
            }
        }
        root.subcommands.extend(node.subcommands);
    }
    root.subcommands.sort_by(|a, b| a.name.cmp(&b.name));

    for (alias_name, target_name) in &alias_mappings {
        if let Ok(new_target_idx) = root
            .subcommands
            .binary_search_by(|c| c.name.as_str().cmp(target_name))
            && let Ok(alias_idx) = root
                .subcommands
                .binary_search_by(|c| c.name.as_str().cmp(alias_name))
        {
            root.subcommands[alias_idx].target = Some(new_target_idx as u32);
        }
    }

    let mut seen = std::collections::HashSet::new();
    for cmd in &root.subcommands {
        if !seen.insert(&cmd.name) {
            return Err(anyhow::anyhow!(
                "Duplicate top-level command '{}'. Different completion scripts cannot define the same command name. Use import to combine them into one file.",
                cmd.name
            ).into());
        }
    }
    let mut cache = CompletionCache {
        root,
        ..Default::default()
    };

    // ── 构建 bytecodes + dyn_index ──
    let mut bytecode_by_idx: Vec<Vec<u8>> = vec![Vec::new(); dynamic_results.len()];
    for (idx, bc) in dyn_cached {
        bytecode_by_idx[idx] = bc;
    }
    for (idx, bc) in fresh_bytecodes {
        bytecode_by_idx[idx] = bc;
    }

    for (dyn_idx, (_, _, func_ids)) in dynamic_results.iter().enumerate() {
        let bc_idx = cache.bytecodes.len() as u32;
        cache.bytecodes.push(bytecode_by_idx[dyn_idx].clone());
        for func_id in func_ids {
            cache.dyn_index.push((func_id.clone(), bc_idx));
        }
    }
    cache.dyn_index.sort_by(|a, b| a.0.cmp(&b.0));

    // ── 序列化 ──
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
    Ok(())
}

pub async fn run_watch(
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
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
    }

    let canonical = dir_path.canonicalize()?;
    println!(
        "{} Watching {}",
        sugg_core::ICON_SCAN,
        sugg_core::path_to_slash(&canonical)
    );

    let mut file_cache: HashMap<PathBuf, crate::CachedFile> = HashMap::new();
    run_build_with_cache(
        Some(dir_path.clone()),
        lang.clone(),
        None,
        None,
        &mut file_cache,
    )
    .await?;

    let (file_tx, mut file_rx) = tokio::sync::mpsc::unbounded_channel();
    let watch_path = dir_path.clone();

    std::thread::spawn(move || {
        let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();
        let mut debouncer = match new_debouncer(
            Duration::from_millis(300),
            move |result: DebounceEventResult| {
                let _ = tx.send(result);
            },
        ) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to create debouncer: {e}");
                return;
            }
        };
        if let Err(e) = debouncer
            .watcher()
            .watch(&watch_path, notify::RecursiveMode::Recursive)
        {
            eprintln!("Failed to watch directory: {e}");
            return;
        }
        for result in rx {
            match result {
                Ok(events) => {
                    let _ = file_tx.send(events);
                }
                Err(e) => eprintln!("Watch error: {e:?}"),
            }
        }
    });

    while let Some(events) = file_rx.recv().await {
        let changed_paths: Vec<PathBuf> = events
            .iter()
            .filter_map(|e| {
                let ext = e.path.extension()?.to_str()?;
                if matches!(ext, "ts" | "js" | "json") {
                    Some(e.path.clone())
                } else {
                    None
                }
            })
            .collect();
        if changed_paths.is_empty() {
            continue;
        }

        for changed in &changed_paths {
            if !changed.exists() {
                file_cache.remove(changed);
            }
            println!(
                "\n{} Changed: {}",
                sugg_core::ICON_BUILD,
                changed.file_name().unwrap().to_string_lossy()
            );
        }
        let start = std::time::Instant::now();
        if let Err(e) = run_build_with_cache(
            Some(dir_path.clone()),
            lang.clone(),
            None,
            None,
            &mut file_cache,
        )
        .await
        {
            log_error!("Rebuild failed: {:#}", e);
        } else {
            println!(
                "{} Hot Reloaded in {:?}",
                sugg_core::ICON_SUCCESS,
                start.elapsed()
            );
        }
    }

    Ok(())
}
