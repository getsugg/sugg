use std::path::{Path, PathBuf};

pub async fn run_upgrade() -> Result<(), Box<dyn std::error::Error>> {
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
