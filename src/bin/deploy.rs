//! 部署脚本：编译 sugg + sugg-engine，安装到 sugg_root() 目录。
//!
//! 布局：
//!   sugg_root()/bin/                ← 加 PATH，只放 sugg.exe
//!   sugg_root()/sugg-engine         ← 内部引擎，不放 bin/ 避免污染 PATH
//!
//! 用法:
//!   cargo deploy --release              # 编译 + 安装
//!   cargo deploy --release --add-path   # 编译 + 安装 + 配置 PATH
//!
//! 安装目录（可通过 SUGG_HOME 环境变量覆盖）:
//!   Windows:  %APPDATA%/sugg/
//!   Linux:    ~/.local/share/sugg/
//!   macOS:    ~/Library/Application Support/sugg/

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_release = args.iter().any(|a| a == "--release");
    let add_path = args.iter().any(|a| a == "--add-path");
    let profile = if is_release { "release" } else { "debug" };

    // ── 1. 构建 sugg + sugg-engine ──
    println!("🔨 Building sugg and sugg-engine ({profile} mode)...");
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--bin", "sugg", "--bin", "sugg-engine"])
        .current_dir(&manifest_dir);
    if is_release {
        cmd.arg("--release");
    }
    let status = cmd.status().expect("Failed to start cargo build");
    if !status.success() {
        eprintln!("❌ Build failed");
        std::process::exit(1);
    }

    // ── 2. 确定编译产物路径 ──
    let sugg_name = if cfg!(windows) { "sugg.exe" } else { "sugg" };
    let sugg_engine_name = if cfg!(windows) {
        "sugg-engine.exe"
    } else {
        "sugg-engine"
    };

    let target_dir = manifest_dir.join("target").join(profile);
    let sugg_src = target_dir.join(sugg_name);
    let sugg_engine_src = target_dir.join(sugg_engine_name);

    if !sugg_src.exists() {
        eprintln!("❌ Build artifact not found: {}", sugg_src.display());
        std::process::exit(1);
    }
    if !sugg_engine_src.exists() {
        eprintln!("❌ Build artifact not found: {}", sugg_engine_src.display());
        std::process::exit(1);
    }

    // ── 3. 安装 ──
    //     bin/                  ← 用户加 PATH，只放 sugg.exe
    //     sugg_root() 根目录      ← 放 sugg-engine.exe，内部查找不暴露
    let sugg_root = if let Ok(var) = std::env::var("SUGG_HOME") {
        PathBuf::from(var)
    } else {
        dirs_next::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sugg")
    };

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

    println!("✅ Installation complete");
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
        println!("💡 Tip: add the following directory to PATH to use sugg globally:");
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
                println!("✅ Added {} to user PATH", dir_str);
                println!("⚠️  Please restart terminal or re-login for changes to take effect");
            }
            "EXISTS" => {
                println!(
                    "ℹ️  PATH already contains {}, no need to add again",
                    dir_str
                );
            }
            _ => {
                eprintln!(
                    "⚠️  PATH configuration failed (PowerShell output: {stdout}), please add manually"
                );
                eprintln!("   Directory: {}", dir_str);
            }
        }
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        // Unix：检测 shell 并写入对应的 profile 文件
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
                    "ℹ️  PATH config already exists in {}, skipping",
                    path.display()
                );
                added = true;
                continue;
            }
            if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
                writeln!(file, "\n# added by sugg deploy\n{export_line}").ok();
                println!("✅ PATH config written to {}", path.display());
                added = true;
            }
        }

        if added {
            println!("⚠️  Run `source ~/.bashrc` (or the appropriate profile) or restart terminal");
        } else {
            eprintln!(
                "⚠️  No shell profile file found. Please manually add the following line to your shell config:"
            );
            eprintln!("   {export_line}");
        }
    }
}
