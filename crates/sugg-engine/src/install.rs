use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

pub const REGISTRY_URL: &str = "https://getsugg.github.io/sugg-completions/registry.json";
pub const RAW_BASE: &str =
    "https://raw.githubusercontent.com/getsugg/sugg-completions/master/completions";

const MAX_CONCURRENT: usize = 8;
const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 500;

#[derive(Deserialize)]
struct Registry {
    scripts: Vec<ScriptEntry>,
}

#[derive(Deserialize)]
struct ScriptEntry {
    name: String,
    description: String,
    source: String,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    i18n: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run_install(
    scripts: Vec<String>,
    list: bool,
    all: bool,
    force: bool,
    langs: &[String],
    completions_dir: &Path,
    registry_url: &str,
    raw_base: &str,
) -> anyhow::Result<()> {
    let url = registry_url;
    let base = raw_base;
    println!("{} Fetching registry from {}...", sugg_core::ICON_SCAN, url);

    let registry = fetch_registry(url).await?;

    if list {
        list_scripts(&registry);
        return Ok(());
    }

    let target_scripts = if all {
        registry.scripts.iter().map(|s| s.name.as_str()).collect()
    } else if scripts.is_empty() {
        return Err(anyhow::anyhow!(
            "No scripts specified. Use --list to see available scripts, or --all to install all."
        ));
    } else {
        scripts.iter().map(|s| s.as_str()).collect::<Vec<_>>()
    };

    // Validate all script names exist in registry
    for name in &target_scripts {
        if registry.scripts.iter().all(|s| s.name != *name) {
            return Err(anyhow::anyhow!(
                "Script '{}' not found in registry. Use --list to see available scripts.",
                name
            ));
        }
    }

    // Detect locale for i18n
    let preferred_langs = if langs.is_empty() {
        vec![crate::detect_locale()]
    } else {
        langs.to_vec()
    };

    // Collect all fallback chains and deduplicate
    let mut all_fallbacks: Vec<String> = preferred_langs
        .iter()
        .flat_map(|lang| crate::get_fallback_chain(lang))
        .collect();
    all_fallbacks.sort();
    all_fallbacks.dedup();

    // Collect all files to download
    let mut downloads = Vec::new();
    let mut downloaded_deps = HashSet::new();

    for name in &target_scripts {
        let entry = registry.scripts.iter().find(|s| s.name == *name).unwrap();

        // Main script file
        downloads.push(entry.source.clone());

        // Shared module dependencies
        for dep in &entry.deps {
            if downloaded_deps.contains(dep) {
                continue;
            }
            downloads.push(dep.clone());
            downloaded_deps.insert(dep.clone());
        }

        // Matching i18n files
        for lang_code in &entry.i18n {
            if all_fallbacks.iter().any(|fb| fb == lang_code) {
                downloads.push(format!("{}/i18n/{}.json", entry.name, lang_code));
            }
        }
    }

    let total = downloads.len();
    let pb = ProgressBar::new(total as u64);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} files")
            .unwrap()
            .progress_chars("█░"),
    );
    pb.println(format!(
        "{} Downloading {} file(s)...",
        sugg_core::ICON_DOWNLOAD,
        total
    ));

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
    let mut set = JoinSet::new();

    for file in downloads {
        let sem = semaphore.clone();
        let dir = completions_dir.to_path_buf();
        let base = base.to_owned();
        let pb = pb.clone();

        set.spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            let result = download_with_retry(&file, &dir, &base, force).await;
            pb.inc(1);
            match result {
                Ok(()) => Ok(file),
                Err(e) => Err(e),
            }
        });
    }

    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(file)) => {
                pb.println(format!("  {} {}", sugg_core::ICON_SUCCESS, file));
            }
            Ok(Err(e)) => {
                pb.finish_and_clear();
                eprintln!("{} {}", sugg_core::ICON_ERROR, e);
                return Err(e);
            }
            Err(e) => {
                pb.finish_and_clear();
                return Err(anyhow::anyhow!("Task failed: {}", e));
            }
        }
    }
    pb.finish_and_clear();

    // Run reload with the first language
    println!("\n{} Rebuilding completion cache...", sugg_core::ICON_BUILD);
    crate::build::run_build(
        Some(completions_dir.to_path_buf()),
        preferred_langs.first().cloned(),
        None,
        None,
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("\n{} Installation complete!", sugg_core::ICON_SUCCESS);
    Ok(())
}

async fn fetch_registry(url: &str) -> anyhow::Result<Registry> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("sugg-installer/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to fetch registry: HTTP {}",
            response.status()
        ));
    }

    let text = response.text().await?;
    let registry: Registry = serde_json::from_str(&text)?;
    Ok(registry)
}

fn list_scripts(registry: &Registry) {
    println!(
        "\n{} Available scripts ({}):\n",
        sugg_core::ICON_STAR,
        registry.scripts.len()
    );

    // Calculate max name length for alignment
    let max_name_len = registry
        .scripts
        .iter()
        .map(|s| s.name.len())
        .max()
        .unwrap_or(0);

    for script in &registry.scripts {
        let i18n_info = if script.i18n.is_empty() {
            String::new()
        } else {
            format!(" [{}]", script.i18n.join(", "))
        };
        println!(
            "  {:width$}  {}{}",
            script.name,
            script.description,
            i18n_info,
            width = max_name_len
        );
    }

    println!(
        "\n{} Use 'sugg install <name>' to install, or 'sugg install --all' to install all.",
        sugg_core::ICON_POINTER
    );
}

async fn download_with_retry(
    relative_path: &str,
    completions_dir: &Path,
    base_url: &str,
    force: bool,
) -> anyhow::Result<()> {
    let mut last_error = None;

    for attempt in 0..=MAX_RETRIES {
        match download_file(relative_path, completions_dir, base_url, force).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    let delay = BASE_DELAY_MS * 2u64.pow(attempt);
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }

    Err(last_error.unwrap())
}

async fn download_file(
    relative_path: &str,
    completions_dir: &Path,
    base_url: &str,
    force: bool,
) -> anyhow::Result<()> {
    let dest = completions_dir.join(relative_path);

    // Create parent directory if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Skip if already exists (unless --force)
    if dest.exists() && !force {
        println!(
            "  {} '{}' already exists, skipping.",
            sugg_core::ICON_INFO,
            relative_path
        );
        return Ok(());
    }

    let url = format!("{}/{}", base_url, relative_path);

    let client = reqwest::Client::builder()
        .user_agent(concat!("sugg-installer/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Failed to download '{}': HTTP {}",
            relative_path,
            response.status()
        ));
    }

    let mut file = std::fs::File::create(&dest)?;
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        use std::io::Write;
        file.write_all(&chunk)?;
    }

    Ok(())
}
