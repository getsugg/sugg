use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

const REGISTRY_URL: &str = "https://getsugg.github.io/sugg-completions/registry.json";
const RAW_BASE: &str =
    "https://raw.githubusercontent.com/getsugg/sugg-completions/master/completions";

#[derive(Deserialize)]
struct Registry {
    #[allow(dead_code)]
    version: String,
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

pub async fn run_install(
    scripts: Vec<String>,
    list: bool,
    all: bool,
    langs: &[String],
    completions_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} Fetching registry from {}...",
        sugg_core::ICON_SCAN,
        REGISTRY_URL
    );

    let registry = fetch_registry().await?;

    if list {
        list_scripts(&registry);
        return Ok(());
    }

    let target_scripts = if all {
        registry.scripts.iter().map(|s| s.name.as_str()).collect()
    } else if scripts.is_empty() {
        eprintln!(
            "{} No scripts specified. Use --list to see available scripts, or --all to install all.",
            sugg_core::ICON_WARN
        );
        std::process::exit(1);
    } else {
        scripts.iter().map(|s| s.as_str()).collect::<Vec<_>>()
    };

    // Validate all script names exist in registry
    for name in &target_scripts {
        if registry.scripts.iter().all(|s| s.name != *name) {
            return Err(format!(
                "Script '{}' not found in registry. Use --list to see available scripts.",
                name
            )
            .into());
        }
    }

    // Detect locale for i18n
    let preferred_langs = if langs.is_empty() {
        vec![sugg_engine::detect_locale()]
    } else {
        langs.to_vec()
    };

    // Collect all fallback chains and deduplicate
    let mut all_fallbacks: Vec<String> = preferred_langs
        .iter()
        .flat_map(|lang| sugg_engine::get_fallback_chain(lang))
        .collect();
    all_fallbacks.sort();
    all_fallbacks.dedup();

    // Track downloaded deps to avoid duplicates
    let mut downloaded_deps = HashSet::new();

    println!(
        "{} Installing {} script(s) to {}...",
        sugg_core::ICON_PACKAGE,
        target_scripts.len(),
        completions_dir.display()
    );

    for name in &target_scripts {
        let entry = registry.scripts.iter().find(|s| s.name == *name).unwrap();

        println!(
            "\n{} Installing '{}' ({})...",
            sugg_core::ICON_DOWNLOAD,
            entry.name,
            entry.description
        );

        // Download main script file
        download_file(&entry.source, completions_dir).await?;

        // Download shared module dependencies
        for dep in &entry.deps {
            if downloaded_deps.contains(dep) {
                println!(
                    "  {} Shared module '{}' already downloaded, skipping.",
                    sugg_core::ICON_INFO,
                    dep
                );
                continue;
            }
            download_file(dep, completions_dir).await?;
            downloaded_deps.insert(dep.clone());
        }

        // Download matching i18n files
        for lang_code in &entry.i18n {
            if all_fallbacks.iter().any(|fb| fb == lang_code) {
                let i18n_path = format!("{}/i18n/{}.json", entry.name, lang_code);
                download_file(&i18n_path, completions_dir).await?;
            }
        }
    }

    // Run reload with the first language
    println!("\n{} Rebuilding completion cache...", sugg_core::ICON_BUILD);
    crate::build::run_build(
        Some(completions_dir.to_path_buf()),
        preferred_langs.first().cloned(),
        None,
        None,
    )
    .await?;

    println!("\n{} Installation complete!", sugg_core::ICON_SUCCESS);
    Ok(())
}

async fn fetch_registry() -> Result<Registry, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("sugg-installer/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let response = client.get(REGISTRY_URL).send().await?;
    if !response.status().is_success() {
        return Err(format!("Failed to fetch registry: HTTP {}", response.status()).into());
    }

    let registry: Registry = response.json().await?;
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

async fn download_file(
    relative_path: &str,
    completions_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let dest = completions_dir.join(relative_path);

    // Create parent directory if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Skip if already exists
    if dest.exists() {
        println!(
            "  {} '{}' already exists, skipping.",
            sugg_core::ICON_INFO,
            relative_path
        );
        return Ok(());
    }

    let url = format!("{}/{}", RAW_BASE, relative_path);

    let client = reqwest::Client::builder()
        .user_agent(concat!("sugg-installer/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(format!(
            "Failed to download '{}': HTTP {}",
            relative_path,
            response.status()
        )
        .into());
    }

    let total_size = response.content_length().unwrap_or(0);

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "  {} {{spinner:.green}} [{{wide_bar:.cyan/blue}}] {{bytes}}/{{total_bytes}}",
                relative_path
            ))?
            .progress_chars("█░"),
    );

    let mut file = std::fs::File::create(&dest)?;
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        use std::io::Write;
        file.write_all(&chunk)?;
        pb.inc(chunk.len() as u64);
    }
    pb.finish_and_clear();

    println!(
        "  {} Downloaded '{}'",
        sugg_core::ICON_SUCCESS,
        relative_path
    );

    Ok(())
}
