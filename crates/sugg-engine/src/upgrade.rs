use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

pub async fn run_upgrade() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{} Checking for the latest version...",
        sugg_core::ICON_SCAN
    );

    let client = reqwest::Client::builder()
        .user_agent(concat!("sugg-updater/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let release_response = client
        .head("https://github.com/getsugg/sugg/releases/latest")
        .send()
        .await?;
    if !release_response.status().is_success() {
        return Err(format!(
            "Failed to fetch release info: HTTP {}",
            release_response.status()
        )
        .into());
    }
    let latest_tag = release_response
        .url()
        .path_segments()
        .and_then(|mut s| s.next_back())
        .and_then(|t| t.strip_prefix("v"))
        .ok_or("Could not determine latest version from GitHub redirect.")?;

    let current = env!("CARGO_PKG_VERSION");

    if !is_newer_version(latest_tag, current) {
        println!(
            "{} Already up-to-date (v{}).",
            sugg_core::ICON_SUCCESS,
            current
        );
        return Ok(());
    }
    println!(
        "{} Upgrading v{} → v{}...",
        sugg_core::ICON_UPGRADE,
        current,
        latest_tag
    );

    let (asset_name, is_zip) = if cfg!(target_os = "windows") {
        ("sugg-x86_64-pc-windows-msvc.zip", true)
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        ("sugg-aarch64-apple-darwin.tar.gz", false)
    } else if cfg!(target_os = "macos") {
        ("sugg-x86_64-apple-darwin.tar.gz", false)
    } else {
        ("sugg-x86_64-unknown-linux-musl.tar.gz", false)
    };

    let download_url = format!(
        "https://github.com/getsugg/sugg/releases/latest/download/{}",
        asset_name
    );

    let total_size = client
        .head(&download_url)
        .send()
        .await?
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let tmp_dir = tempfile::tempdir()?;
    let tmp_path = tmp_dir.path();
    let archive_path = tmp_path.join(if is_zip { "sugg.zip" } else { "sugg.tar.gz" });
    let extract_dir = tmp_path.join("extract");
    std::fs::create_dir_all(&extract_dir)?;

    println!(
        "{} Downloading from {}...",
        sugg_core::ICON_DOWNLOAD,
        download_url
    );

    let mut response = client.get(&download_url).send().await?;
    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()).into());
    }

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?
            .progress_chars("█░"),
    );

    let mut file = std::fs::File::create(&archive_path)?;
    while let Some(chunk) = response.chunk().await? {
        use std::io::Write;
        file.write_all(&chunk)?;
        pb.inc(chunk.len() as u64);
    }
    pb.finish_and_clear();
    drop(file);

    println!("{} Extracting binaries...", sugg_core::ICON_PACKAGE);
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

    fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
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

    println!("{} Replacing binaries...", sugg_core::ICON_SYNC);
    let sugg_root = sugg_core::sugg_root();
    let backup_dir = sugg_root.join(".old");
    std::fs::create_dir_all(&backup_dir)?;

    let sugg_dest = sugg_root.join("bin").join(sugg_name);
    let engine_dest = sugg_root.join(engine_name);

    replace_binary_safe(&new_sugg, &sugg_dest, &backup_dir)?;
    replace_binary_safe(&new_engine, &engine_dest, &backup_dir)?;

    println!("{} Upgrade complete!", sugg_core::ICON_SUCCESS);
    Ok(())
}

fn replace_binary_safe(
    new_path: &Path,
    dest_path: &Path,
    backup_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(p) = dest_path.parent() {
        std::fs::create_dir_all(p)?;
    }

    let backup = backup_dir.join(dest_path.file_name().unwrap());
    let _ = std::fs::remove_file(&backup);

    if dest_path.exists() {
        std::fs::rename(dest_path, &backup)?;
        std::fs::copy(new_path, dest_path)?;
    } else {
        std::fs::copy(new_path, dest_path)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dest_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(dest_path, perms)?;
    }

    Ok(())
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse::<u64>().ok()).collect() };
    let latest_parts = parse(latest);
    let current_parts = parse(current);
    for i in 0..latest_parts.len().max(current_parts.len()) {
        let l = latest_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        } else if l < c {
            return false;
        }
    }
    false
}
