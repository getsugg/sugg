//! Deploy script: build sugg + sugg-engine and install them into sugg_root().
//!
//! Layout:
//!   sugg_root()/bin/                <- added to PATH, contains only sugg.exe
//!   sugg_root()/sugg-engine         <- internal engine, kept out of bin/ to avoid polluting PATH
//!
//! Usage:
//!   cargo deploy --release                # build + install
//!   cargo deploy --release --add-path     # build + install + configure PATH
//!   cargo deploy --release --no-build     # skip build (already built by cargo test), install directly
//!   cargo deploy --release --no-build --add-path
//!
//! Install directory (overridable via the SUGG_HOME env var):
//!   ~/.sugg/

use std::path::PathBuf;
use std::process::Command;

// =========================================================================
// Local copies of icon constants and path helpers (fully cuts the dependency on sugg-core)
// =========================================================================
const ICON_BUILD: &str = "🔧";
const ICON_ERROR: &str = "❌";
const ICON_SUCCESS: &str = "✅";
#[cfg(windows)]
const ICON_WARN: &str = "❗";
const ICON_INFO: &str = "💡";

fn sugg_root() -> PathBuf {
    if let Ok(var) = std::env::var("SUGG_HOME") {
        return PathBuf::from(var);
    }
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".sugg")
}

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_release = args.iter().any(|a| a == "--release");
    let add_path = args.iter().any(|a| a == "--add-path");
    let skip_build = args.iter().any(|a| a == "--no-build");
    let profile = if is_release { "release" } else { "debug" };

    // ── 1. Build sugg + sugg-engine (--no-build skips this, installing existing artifacts) ──
    if !skip_build {
        println!(
            "{} Building sugg and sugg-engine ({profile} mode)...",
            ICON_BUILD
        );
        let mut cmd = Command::new("cargo");
        cmd.args(["build", "-p", "sugg", "-p", "sugg-engine"])
            .current_dir(&manifest_dir);
        if is_release {
            cmd.arg("--release");
        }
        let status = cmd.status().expect("Failed to start cargo build");
        if !status.success() {
            eprintln!("{} Build failed", ICON_ERROR);
            std::process::exit(1);
        }
    }

    // ── 2. Determine build artifact paths ──
    let sugg_name = if cfg!(windows) { "sugg.exe" } else { "sugg" };
    let sugg_engine_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };

    // In a workspace the target dir lives at the workspace root, not inside the crate
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let target_dir = workspace_root.join("target").join(profile);
    let sugg_src = target_dir.join(sugg_name);
    let sugg_engine_src = target_dir.join(sugg_engine_name);

    if !sugg_src.exists() {
        eprintln!(
            "{} Build artifact not found: {}",
            ICON_ERROR,
            sugg_src.display()
        );
        std::process::exit(1);
    }
    if !sugg_engine_src.exists() {
        eprintln!(
            "{} Build artifact not found: {}",
            ICON_ERROR,
            sugg_engine_src.display()
        );
        std::process::exit(1);
    }

    // ── 3. Install ──
    let sugg_root = sugg_root();

    let bin_dir = sugg_root.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("Failed to create bin directory");

    let sugg_dst = bin_dir.join(sugg_name);
    let sugg_engine_dst = sugg_root.join(sugg_engine_name);

    std::fs::copy(&sugg_src, &sugg_dst).unwrap_or_else(|e| {
        panic!(
            "Copy {} -> {} failed: {e}",
            sugg_src.display(),
            sugg_dst.display()
        )
    });
    std::fs::copy(&sugg_engine_src, &sugg_engine_dst).unwrap_or_else(|e| {
        panic!(
            "Copy {} -> {} failed: {e}",
            sugg_engine_src.display(),
            sugg_engine_dst.display()
        )
    });

    println!("{} Installation complete", ICON_SUCCESS);
    println!(
        "   sugg          -> {}   ← added to PATH",
        sugg_dst.display()
    );
    println!(
        "   sugg-engine   -> {}   ← internal use, not exposed",
        sugg_engine_dst.display()
    );
    println!();

    // ── 4. PATH configuration (only adds bin/, so PATH is not polluted) ──
    if add_path {
        add_dir_to_path(&bin_dir);
    } else {
        println!(
            "{} Tip: add the following directory to PATH to use sugg globally:",
            ICON_INFO
        );
        println!("       {}", bin_dir.display());
        println!("   Next time, use --add-path to auto-configure:");
        println!("       cargo deploy --release --add-path");
    }
}

/// Add the given directory to the system PATH environment variable
fn add_dir_to_path(bin_dir: &std::path::Path) {
    #[cfg(windows)]
    {
        // Windows: set the user-level PATH via PowerShell
        let dir_str = bin_dir.to_string_lossy().replace('/', "\\");
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "$p = [Environment]::GetEnvironmentVariable('Path', 'User'); \
                     if ($p -split ';' -notcontains '{dir_str}') {{ \
                       [Environment]::SetEnvironmentVariable('Path', \"$p;{dir_str}\", 'User'); \
                       Write-Output 'ADDED' \
                     }} else {{ \
                       Write-Output 'EXISTS' \
                     }}"
                ),
            ])
            .output()
            .expect("Failed to execute PowerShell");

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        match stdout.as_str() {
            "ADDED" => {
                println!("{} Added {} to user PATH", ICON_SUCCESS, dir_str);
                println!(
                    "{} Please restart terminal or re-login for changes to take effect",
                    ICON_INFO
                );
            }
            "EXISTS" => {
                println!(
                    "{} PATH already contains {}, no need to add again",
                    ICON_INFO, dir_str
                );
            }
            _ => {
                eprintln!(
                    "{} PATH configuration failed (PowerShell output: {stdout}), please add manually",
                    ICON_WARN
                );
                eprintln!("   Directory: {}", dir_str);
            }
        }
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        let shell = std::env::var("SHELL").unwrap_or_default();
        let export_line = format!(r#"export PATH="{}:$PATH""#, bin_dir.display());

        let profile_files: &[&str] = if shell.ends_with("zsh") {
            &[".zshrc", ".zprofile", ".zshenv"]
        } else if shell.ends_with("bash") {
            &[".bashrc", ".bash_profile", ".profile"]
        } else {
            &[".profile"]
        };

        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let mut added = false;

        for fname in profile_files {
            let path = std::path::Path::new(&home).join(fname);
            if !path.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            if content.contains(&export_line) {
                println!(
                    "{} PATH config already exists in {}, skipping",
                    ICON_INFO,
                    path.display()
                );
                added = true;
                continue;
            }
            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
                writeln!(file, "\n# added by sugg deploy\n{export_line}").ok();
                println!("{} PATH config written to {}", ICON_SUCCESS, path.display());
                added = true;
            }
        }

        if added {
            println!(
                "{} Run `source ~/.bashrc` (or the appropriate profile) or restart terminal",
                ICON_INFO
            );
        } else {
            eprintln!(
                "{} No shell profile file found. Please manually add the following line to your shell config:",
                ICON_INFO
            );
            eprintln!("   {export_line}");
        }
    }
}
