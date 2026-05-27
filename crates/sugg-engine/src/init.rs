use include_dir::{Dir, include_dir};
use std::fs;
use std::path::Path;

static ASSETS_INIT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/init");
static ASSETS_SHELL_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets/shell");

/// 将嵌入目录中的所有文件递归写出到 dest，返回被跳过（已存在）的文件路径列表
fn extract_dir(
    dir: &Dir,
    dest: &Path,
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

pub fn run_init(shell_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let shell = shell_name
        .parse::<sugg_core::Shell>()
        .map_err(|e| e.to_string())?;
    let file_name = match shell {
        sugg_core::Shell::Bash => "bash.sh",
        sugg_core::Shell::Zsh => "zsh.zsh",
        sugg_core::Shell::Fish => "fish.fish",
        sugg_core::Shell::Nushell => "nushell.nu",
        sugg_core::Shell::Powershell => "powershell.ps1",
    };
    let file = ASSETS_SHELL_DIR
        .get_file(file_name)
        .ok_or_else(|| format!("No bridge script found for shell '{}'", file_name))?;
    let content = file
        .contents_utf8()
        .ok_or_else(|| format!("Bridge script for '{}' is not valid UTF-8", file_name))?;

    // 将占位符替换为 sugg 可执行文件的绝对路径（位于 sugg-engine 所在目录下的 bin 子目录中）
    // nushell 中反斜杠需转为正斜杠
    let sugg_bin = std::env::current_exe()
        .ok()
        .map(|p| {
            let exe = if cfg!(windows) { "sugg.exe" } else { "sugg" };
            // p 通常为 .../sugg/sugg-engine[.exe]，需要定位到 .../sugg/bin/sugg[.exe]
            p.parent()
                .expect("current_exe should have a parent directory")
                .join("bin")
                .join(exe)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .unwrap_or_else(|| "sugg".to_string());
    let content = content.replace("{{SUGG_BIN}}", &sugg_bin);

    print!("{}", content);

    if console::user_attended() {
        use console::style;
        use sugg_core::TerminalBox;

        let mut banner = TerminalBox::new()
            .border_color(console::Style::new().bold().cyan())
            .line(
                style(format!(
                    "{} Sugg {} shell integration generated successfully!",
                    sugg_core::ICON_PARTY,
                    shell.as_str(),
                ))
                .green()
                .to_string(),
            )
            .empty_line()
            .line(
                style(format!(
                    "{} To apply this automatically on shell startup,",
                    sugg_core::ICON_POINTER
                ))
                .bold()
                .to_string(),
            )
            .line(
                style("  add the following to your config:")
                    .bold()
                    .to_string(),
            );

        match shell {
            sugg_core::Shell::Bash => {
                banner = banner.line(style("    eval \"$(sugg init bash)\"").yellow().to_string());
            }
            sugg_core::Shell::Zsh => {
                banner = banner.line(style("    eval \"$(sugg init zsh)\"").yellow().to_string());
            }
            sugg_core::Shell::Fish => {
                banner = banner.line(style("    sugg init fish | source").yellow().to_string());
            }
            sugg_core::Shell::Nushell => {
                banner = banner
                    .line(
                        style(
                            &format!("    {}  Recommended (Nushell 0.102+ — no config editing needed):", sugg_core::ICON_SPARKLES),
                        )
                        .cyan()
                        .to_string(),
                    )
                    .line(
                        style(
                            "      mkdir ($nu.default-config-dir | path join 'autoload')",
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            "      sugg init nushell | save -f ($nu.default-config-dir | path join 'autoload/sugg.nu')",
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line("")
                    .line(
                        style(
                            "    Legacy (edit config.nu — script stored in sugg's own directory):",
                        )
                        .cyan()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  let sugg_dir = if ('SUGG_HOME' in $env) { $env.SUGG_HOME } else { ($nu.data-dir | path join 'sugg') }"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  mkdir ($sugg_dir | path join 'shells')"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  sugg init nushell | save -f ($sugg_dir | path join 'shells/nushell.nu')"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  # Then add to your config.nu:"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  source ($sugg_dir | path join 'shells/nushell.nu')"#,
                        )
                        .yellow()
                        .to_string(),
                    );
            }
            sugg_core::Shell::Powershell => {
                banner = banner
                    .line(
                        style(format!(
                            "{}  Save as static script (recommended, lower startup overhead):",
                            sugg_core::ICON_SPARKLES,
                        ))
                        .cyan()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  $suggDir = if ($env:SUGG_HOME) { $env:SUGG_HOME } else { "$env:APPDATA\sugg" }"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  New-Item -ItemType Directory -Path (Join-Path $suggDir "shells") -Force | Out-Null"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  sugg init powershell | Out-File -FilePath (Join-Path $suggDir "shells\powershell.ps1") -Encoding utf8"#,
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  # Then add to your $PROFILE:"#,
                        )
                        .green()
                        .to_string(),
                    )
                    .line(
                        style(
                            r#"  . (Join-Path (if ($env:SUGG_HOME) { $env:SUGG_HOME } else { "$env:APPDATA\sugg" }) "shells\powershell.ps1")"#,
                        )
                        .yellow()
                        .to_string(),
                    );
            }
        }

        banner.print();
    }

    Ok(())
}

pub fn run_dev_init(completions_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let system_dir = completions_dir.join(".sugg");
    if system_dir.exists() {
        fs::remove_dir_all(&system_dir)?;
    }
    let sugg_asset_dir = ASSETS_INIT_DIR
        .get_dir(".sugg")
        .expect("assets/init/.sugg missing");
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

    let skipped = extract_dir(&ASSETS_INIT_DIR, completions_dir, true)?;
    for path in skipped.iter().filter(|p| !p.starts_with(&system_dir)) {
        println!(
            "{}  {} already exists, skipped.",
            sugg_core::ICON_INFO,
            sugg_core::path_to_slash(path)
        );
    }

    println!(
        "{} Dev environment initialized at {}",
        sugg_core::ICON_SUCCESS,
        sugg_core::path_to_slash(&system_dir)
    );
    Ok(())
}
