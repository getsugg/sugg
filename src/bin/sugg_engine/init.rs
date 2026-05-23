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
        .parse::<sugg::Shell>()
        .map_err(|e| e.to_string())?;
    let file_name = match shell {
        sugg::Shell::Bash => "bash.sh",
        sugg::Shell::Zsh => "zsh.zsh",
        sugg::Shell::Fish => "fish.fish",
        sugg::Shell::Nushell => "nushell.nu",
        sugg::Shell::Powershell => "powershell.ps1",
    };
    let file = ASSETS_SHELL_DIR
        .get_file(file_name)
        .ok_or_else(|| format!("No bridge script found for shell '{}'", file_name))?;
    let content = file
        .contents_utf8()
        .ok_or_else(|| format!("Bridge script for '{}' is not valid UTF-8", file_name))?;
    print!("{}", content);

    if console::user_attended() {
        use console::style;
        use sugg::TerminalBox;

        let mut banner = TerminalBox::new()
            .border_color(console::Style::new().bold().cyan())
            .line(
                style(format!(
                    "{} Sugg {} shell integration generated successfully!",
                    sugg::ICON_PARTY,
                    shell.as_str(),
                ))
                .green()
                .to_string(),
            )
            .empty_line()
            .line(
                style(format!(
                    "{} To apply this automatically on shell startup,",
                    sugg::ICON_POINTER
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
            sugg::Shell::Bash => {
                banner = banner.line(style("    eval \"$(sugg init bash)\"").yellow().to_string());
            }
            sugg::Shell::Zsh => {
                banner = banner.line(style("    eval \"$(sugg init zsh)\"").yellow().to_string());
            }
            sugg::Shell::Fish => {
                banner = banner.line(style("    sugg init fish | source").yellow().to_string());
            }
            sugg::Shell::Nushell => {
                banner = banner
                    .line(
                        style(
                            &format!("    {}  Recommended (Nushell 0.102+ — no config editing needed):", sugg::ICON_SPARKLES),
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
                    .line(
                        style(
                            "    Legacy (any version — requires editing config.nu):",
                        )
                        .cyan()
                        .to_string(),
                    )
                    .line(
                        style(
                            "      sugg init nushell | save -f ~/.sugg_init.nu",
                        )
                        .yellow()
                        .to_string(),
                    )
                    .line(
                        style(
                            "      (Then add source ~/.sugg_init.nu to your config.nu)",
                        )
                        .yellow()
                        .to_string(),
                    );
            }
            sugg::Shell::Powershell => {
                banner = banner.line(
                    style("    sugg init powershell | Invoke-Expression")
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
    let skipped = extract_dir(&ASSETS_INIT_DIR, completions_dir, true)?;
    for path in skipped.iter().filter(|p| !p.starts_with(&system_dir)) {
        println!(
            "{}  {} already exists, skipped.",
            sugg::ICON_INFO,
            sugg::path_to_slash(path)
        );
    }

    println!(
        "{} Dev environment initialized at {}",
        sugg::ICON_SUCCESS,
        sugg::path_to_slash(&system_dir)
    );
    Ok(())
}
