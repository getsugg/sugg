use include_dir::{Dir, include_dir};
use std::fs;
use std::io::{IsTerminal, stdout};
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

    if stdout().is_terminal() {
        eprintln!("\n# ==========================================================================");
        eprintln!(
            "# 💡 sugg shell integration for {} generated successfully.",
            shell.as_str()
        );
        eprintln!(
            "# To apply this automatically on shell startup, add the following to your config:"
        );
        match shell {
            sugg::Shell::Bash => eprintln!("#   eval \"$(sugg init bash)\""),
            sugg::Shell::Zsh => eprintln!("#   eval \"$(sugg init zsh)\""),
            sugg::Shell::Fish => eprintln!("#   sugg init fish | source"),
            sugg::Shell::Nushell => {
                eprintln!("#   sugg init nushell | save -f ~/.sugg_init.nu");
                eprintln!("#   (Then add `source ~/.sugg_init.nu` to your env.nu/config.nu)");
            }
            sugg::Shell::Powershell => eprintln!("#   sugg init powershell | Invoke-Expression"),
        }
        eprintln!("# ==========================================================================\n");
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
        println!("💡  {} already exists, skipped.", sugg::path_to_slash(path));
    }

    println!(
        "✅ Dev environment initialized at {}",
        sugg::path_to_slash(&system_dir)
    );
    Ok(())
}
