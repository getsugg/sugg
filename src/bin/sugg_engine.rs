use include_dir::{Dir, include_dir};
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
use sugg::log_warn;

fn json_to_command_node(v: JsonValue) -> CommandNode {
    serde_json::from_value(v).unwrap_or_default()
}

struct EngineArgs {
    completions_dir: Option<PathBuf>,
    lang: Option<String>,
    cache_dir: Option<PathBuf>,
    debug_dump_dynamic: Option<PathBuf>,
}

/// 解析子命令后的参数，skip_count 为跳过的前缀参数数量（一级命令传 2，二级命令传 3）
fn parse_engine_args(skip_count: usize) -> EngineArgs {
    let mut completions_dir = None;
    let mut lang = None;
    let mut cache_dir = None;
    let mut debug_dump_dynamic = None;
    let mut parser = lexopt::Parser::from_args(std::env::args().skip(skip_count));
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
            lexopt::Arg::Long("dump-dynamic") => {
                debug_dump_dynamic = parser
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
        debug_dump_dynamic,
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
        .unwrap_or_else(sugg::default_completions_dir);
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
    let (bundled_static, dynamic_bundles) = match sugg::build_bundles(&dir_path, &lang).await {
        Ok(res) => res,
        Err(e) => {
            log_error!("Script Error: {:#}", e);
            return Ok(());
        }
    };

    // 调试导出：将动态 bundle 写入指定目录，便于检查编译后的 JS 代码
    if let Some(dump_dir) = &args.debug_dump_dynamic {
        fs::create_dir_all(dump_dir)?;
        for (stem, code, _) in &dynamic_bundles {
            let out_path = dump_dir.join(format!("{stem}.js"));
            fs::write(&out_path, code)?;
            println!("🔍 Debug dump: {}", out_path.display());
        }
    }

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

async fn run_upgrade() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Checking for the latest version...");

    // 调 GitHub API 获取最新 tag，与编译时版本比较
    let api_output = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "https://api.github.com/repos/axuj/sugg/releases/latest",
        ])
        .output()?;
    if !api_output.status.success() {
        return Err("Failed to fetch release info from GitHub.".into());
    }
    let api_json: serde_json::Value = serde_json::from_slice(&api_output.stdout)?;
    let latest_tag = api_json["tag_name"]
        .as_str()
        .ok_or("Missing tag_name in GitHub API response.")?
        .trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");

    if latest_tag == current {
        println!("✅ Already up-to-date (v{}).", current);
        return Ok(());
    }
    println!("⬆️  Upgrading v{} → v{}...", current, latest_tag);

    let (asset_name, is_zip) = if cfg!(target_os = "windows") {
        ("sugg-x86_64-pc-windows-msvc.zip", true)
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        ("sugg-aarch64-apple-darwin.tar.gz", false)
    } else if cfg!(target_os = "macos") {
        ("sugg-x86_64-apple-darwin.tar.gz", false)
    } else {
        ("sugg-x86_64-unknown-linux-gnu.tar.gz", false)
    };

    let download_url = format!(
        "https://github.com/axuj/sugg/releases/latest/download/{}",
        asset_name
    );

    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path();
    let archive_path = tmp_path.join(if is_zip { "sugg.zip" } else { "sugg.tar.gz" });
    let extract_dir = tmp_path.join("extract");
    std::fs::create_dir_all(&extract_dir)?;

    println!("⬇️  Downloading from {}...", download_url);
    let status = std::process::Command::new("curl")
        .args(["-fL", &download_url, "-o", &archive_path.to_string_lossy()])
        .status()?;
    if !status.success() {
        return Err("Download failed: curl returned non-zero status.".into());
    }

    println!("📦 Extracting binaries...");
    if is_zip {
        let status = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive_path.display(),
                    extract_dir.display()
                ),
            ])
            .status()?;
        if !status.success() {
            return Err("Extraction failed: Expand-Archive returned non-zero status.".into());
        }
    } else {
        let status = std::process::Command::new("tar")
            .args([
                "-xzf",
                &archive_path.to_string_lossy(),
                "-C",
                &extract_dir.to_string_lossy(),
            ])
            .status()?;
        if !status.success() {
            return Err("Extraction failed: tar returned non-zero status.".into());
        }
    }

    fn find_file(dir: &std::path::Path, name: &str) -> Option<PathBuf> {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let p = e.path();
            if p.is_file() && p.file_name()?.to_string_lossy() == name {
                Some(p)
            } else if p.is_dir() {
                find_file(&p, name)
            } else {
                None
            }
        })
    }

    let sugg_name = if cfg!(windows) { "sugg.exe" } else { "sugg" };
    let engine_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };

    let new_sugg = find_file(&extract_dir, sugg_name)
        .ok_or_else(|| format!("Could not find {} in archive.", sugg_name))?;
    let new_engine = find_file(&extract_dir, engine_name)
        .ok_or_else(|| format!("Could not find {} in archive.", engine_name))?;

    println!("🔄 Replacing binaries...");
    let sugg_root = sugg::sugg_root();
    // sugg.exe -> sugg_root/bin/sugg.exe
    let sugg_dest = sugg_root.join("bin").join(sugg_name);
    if let Some(p) = sugg_dest.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(&new_sugg, &sugg_dest)?;

    // sugg-engine.exe -> sugg_root/sugg-engine.exe (self_replace handles Windows file lock)
    self_replace::self_replace(&new_engine)?;

    println!("✅ Upgrade complete!");
    Ok(())
}

/// 将嵌入目录中的所有文件递归写出到 dest，返回被跳过（已存在）的文件路径列表
fn extract_dir(
    dir: &Dir,
    dest: &std::path::Path,
    skip_existing: bool,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut skipped = Vec::new();
    for file in dir.files() {
        let out = dest.join(file.path());
        if skip_existing && out.exists() {
            skipped.push(out);
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out, file.contents())?;
    }
    for subdir in dir.dirs() {
        skipped.extend(extract_dir(subdir, dest, skip_existing)?);
    }
    Ok(skipped)
}

static ASSETS_INIT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/init");

fn run_dev_init(args: &EngineArgs) -> Result<(), Box<dyn std::error::Error>> {
    let completions_dir = args
        .completions_dir
        .clone()
        .or_else(|| {
            std::env::var("SUGG_COMPLETIONS_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .unwrap_or_else(sugg::default_completions_dir);

    // .sugg/ — 每次全量覆盖，先删后写
    let system_dir = completions_dir.join(".sugg");
    if system_dir.exists() {
        fs::remove_dir_all(&system_dir)?;
    }
    let sugg_asset_dir = ASSETS_INIT_DIR
        .get_dir(".sugg")
        .expect("assets/init/.sugg missing");
    // sugg.d.ts 注入版本号，其余文件直接写出
    fs::create_dir_all(&system_dir)?;
    fs::write(
        system_dir.join("sugg.d.ts"),
        format!(
            "// Version: {}\n{}",
            env!("CARGO_PKG_VERSION"),
            sugg_asset_dir
                .get_file(".sugg/sugg.d.ts")
                .map(|f| f.contents_utf8().unwrap_or(""))
                .unwrap_or("")
        ),
    )?;
    for file in sugg_asset_dir
        .files()
        .filter(|f| f.path().file_name().unwrap_or_default() != "sugg.d.ts")
    {
        fs::write(
            system_dir.join(file.path().file_name().unwrap()),
            file.contents(),
        )?;
    }

    // 其余文件 — 仅首次生成，不覆盖用户自定义
    let skipped = extract_dir(&ASSETS_INIT_DIR, &completions_dir, true)?;
    for path in skipped.iter().filter(|p| !p.starts_with(&system_dir)) {
        println!("⚠️  {} already exists, skipped.", sugg::path_to_slash(path));
    }

    println!(
        "✅ Dev environment initialized at {}",
        sugg::path_to_slash(&system_dir)
    );
    Ok(())
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
        .unwrap_or_else(sugg::default_completions_dir);
    if !completions_dir.exists() {
        fs::create_dir_all(&completions_dir).expect("Failed to create completions directory");
    }

    let preferred_lang = args
        .lang
        .clone()
        .or_else(|| std::env::var("SUGG_LANG").ok())
        .unwrap_or_else(|| "en".to_string());

    // 基于 BCP 47 生成回退链，供 JSDoc 智能优先展示最佳匹配翻译
    let fallbacks = sugg::get_fallback_chain(&preferred_lang);

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
                if let Ok(s) = fs::read_to_string(&path)
                    && let Ok(map) =
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

    // 按回退链查找优先展示的翻译，用 🚩 标记
    fn find_best_lang<'a>(
        fallbacks: &[String],
        translations: &'a std::collections::BTreeMap<String, String>,
    ) -> Option<&'a str> {
        for fb in fallbacks.iter().rev() {
            for (lang, _) in translations.iter() {
                if lang.eq_ignore_ascii_case(fb) {
                    return Some(lang.as_str());
                }
            }
        }
        None
    }

    let mut s = String::new();
    if keys_map.is_empty() {
        s.push_str("// No i18n keys found.\n");
    } else {
        for (ns, ns_keys) in &keys_map {
            if ns_keys.is_empty() {
                continue;
            }
            let module_path = format!("virtual:i18n/{}", ns);
            s.push_str(&format!("declare module \"{}\" {{\n", module_path));
            for (key, translations) in ns_keys {
                s.push_str("  /**\n");
                let best_lang = find_best_lang(&fallbacks, translations);
                if best_lang.is_none() {
                    log_warn!(
                        "i18n key '{}' in namespace '{}' has no translation for preferred language '{}' (fallback chain: {}). Available translations: {}",
                        key,
                        ns,
                        preferred_lang,
                        fallbacks.join(", "),
                        translations.keys().cloned().collect::<Vec<_>>().join(", ")
                    );
                }
                if let Some(bl) = best_lang
                    && let Some(text) = translations.get(bl)
                {
                    s.push_str(&format!("   * - 🚩 **{}**: {}\n", bl, text));
                }
                for (lang, text) in translations {
                    if Some(lang.as_str()) == best_lang {
                        continue;
                    }
                    s.push_str(&format!("   * - **{}**: {}\n", lang, text));
                }
                s.push_str("   */\n");
                s.push_str(&format!("  export const {}: string;\n", key));
            }
            s.push_str("}\n\n");
        }
    }

    let sugg_dir = completions_dir.join(".sugg");
    fs::create_dir_all(&sugg_dir).expect("Failed to create .sugg directory");
    let out_path = sugg_dir.join("i18n.d.ts");
    fs::write(&out_path, &s).expect("Failed to write i18n.d.ts");
    println!(
        "✅ Generated {} with {} namespaces.",
        sugg::path_to_slash(&out_path),
        keys_map.len()
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("dev") => {
            let args = parse_engine_args(3);
            match std::env::args().nth(2).as_deref() {
                Some("init") => run_dev_init(&args)?,
                Some("i18n") => run_i18n_gen(&args),
                sub => {
                    eprintln!(
                        "❌ Unknown dev subcommand: {}. Available: init, i18n",
                        sub.unwrap_or_default()
                    );
                    std::process::exit(1);
                }
            }
            Ok(())
        }
        Some("commands") => {
            let cache_path = sugg::cache::get_cache_path();
            if let Ok(data) = std::fs::read(&cache_path)
                && let Ok(archived) =
                    access::<sugg::cache::structs::ArchivedCompletionCache, Error>(&data)
            {
                for cmd in archived.root.subcommands.iter() {
                    println!("{}", cmd.name);
                }
            }
            Ok(())
        }
        Some("reload") => run_build(&parse_engine_args(2)).await,
        Some("upgrade") => {
            if let Err(e) = run_upgrade().await {
                eprintln!("❌ Upgrade failed: {}", e);
                std::process::exit(1);
            }
            Ok(())
        }
        cmd => {
            eprintln!(
                "❌ Unknown command: {}. Available: dev, reload, commands, upgrade",
                cmd.unwrap_or_default()
            );
            std::process::exit(1);
        }
    }
}
