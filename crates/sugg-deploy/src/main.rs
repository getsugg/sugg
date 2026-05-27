//! 部署脚本：编译 sugg + sugg-engine，安装到 sugg_root() 目录。
//!
//! 布局：
//!   sugg_root()/bin/                ← 加 PATH，只放 sugg.exe
//!   sugg_root()/sugg-engine         ← 内部引擎，不放 bin/ 避免污染 PATH
//!
//! 用法:
//!   cargo deploy --release                # 编译 + 安装
//!   cargo deploy --release --add-path     # 编译 + 安装 + 配置 PATH
//!   cargo deploy --release --no-build     # 跳过编译（已经在 cargo test 中编译过了），直接安装
//!   cargo deploy --release --no-build --add-path
//!
//! 安装目录（可通过 SUGG_HOME 环境变量覆盖）:
//!   ~/.sugg/

use std::path::PathBuf;
use std::process::Command;

// =========================================================================
// 本地复制的图标常量与路径函数（彻底切断对 sugg-core 的依赖）
// =========================================================================
const ICON_BUILD: &str = "🔧";
const ICON_ERROR: &str = "❌";
const ICON_SUCCESS: &str = "✅";
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

    // ── 1. 构建 sugg + sugg-engine（--no-build 跳过，直接从已有产物安装）──
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

    // ── 2. 确定编译产物路径 ──
    let sugg_name = if cfg!(windows) { "sugg.exe" } else { "sugg" };
    let sugg_engine_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };

    // 在 workspace 中，target dir 在 workspace root，不在 crate 内
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

    // ── 3. 安装 ──
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

    // ── 4. PATH 配置（只加 bin/，不会污染 PATH）──
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

/// 将指定目录添加到系统 PATH 环境变量
fn add_dir_to_path(bin_dir: &std::path::Path) {
    let dir_str = bin_dir.to_string_lossy().replace('/', "\\");

    #[cfg(windows)]
    {
        // Windows：通过 PowerShell 设置用户级 PATH
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
