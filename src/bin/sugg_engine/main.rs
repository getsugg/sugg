use clap::{Parser, Subcommand};
use rkyv::access;
use rkyv::rancor::Error;
use std::path::PathBuf;

mod build;
mod i18n;
mod init;
mod upgrade;

#[derive(Parser)]
#[command(version, about = "sugg - Shell completion engine", bin_name = "sugg")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Developer tools
    Dev {
        #[command(subcommand)]
        sub: DevCommands,
        /// Path to completions directory
        #[arg(long)]
        completions_dir: Option<PathBuf>,
        /// Preferred language (e.g. en, zh-CN)
        #[arg(long)]
        lang: Option<String>,
    },
    /// List all cached top-level commands
    #[command(name = "commands")]
    CachedList,
    /// Rebuild completion cache from scripts
    Reload {
        /// Path to completions directory
        #[arg(long)]
        completions_dir: Option<PathBuf>,
        /// Preferred language
        #[arg(long)]
        lang: Option<String>,
        /// Override cache file path
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        /// Dump dynamic bundles for debugging
        #[arg(long)]
        dump_dynamic: Option<PathBuf>,
    },
    /// Upgrade sugg to the latest version
    Upgrade,
    /// Print shell integration script
    Init {
        /// Shell name (bash, zsh, fish, nushell, powershell)
        shell: Option<String>,
    },
}

#[derive(Subcommand)]
enum DevCommands {
    /// Initialize dev environment
    Init {
        /// Path to completions directory
        #[arg(long)]
        completions_dir: Option<PathBuf>,
    },
    /// Generate i18n type declarations
    I18n {
        /// Path to completions directory
        #[arg(long)]
        completions_dir: Option<PathBuf>,
        /// Preferred language
        #[arg(long)]
        lang: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Dev {
            sub,
            completions_dir,
            lang,
        } => match sub {
            DevCommands::Init {
                completions_dir: cd,
            } => {
                let dir = cd
                    .or(completions_dir)
                    .or_else(|| {
                        std::env::var("SUGG_COMPLETIONS_DIR")
                            .ok()
                            .map(PathBuf::from)
                    })
                    .unwrap_or_else(sugg::default_completions_dir);
                init::run_dev_init(&dir)?;
                Ok(())
            }
            DevCommands::I18n {
                completions_dir: cd,
                lang: l,
            } => {
                let dir = cd
                    .or(completions_dir)
                    .or_else(|| {
                        std::env::var("SUGG_COMPLETIONS_DIR")
                            .ok()
                            .map(PathBuf::from)
                    })
                    .unwrap_or_else(sugg::default_completions_dir);
                let lang = l
                    .or(lang)
                    .or_else(|| std::env::var("SUGG_LANG").ok())
                    .unwrap_or_else(|| "en".to_string());
                i18n::run_i18n_gen(&dir, &lang);
                Ok(())
            }
        },
        Commands::CachedList => {
            let cache_path = sugg::cache::get_cache_path();
            if let Ok(data) = std::fs::read(&cache_path)
                && let Ok(archived) =
                    access::<sugg::cache::structs::ArchivedCompletionCache, Error>(&data)
            {
                for cmd in archived.root.subcommands.iter() {
                    println!("{}", cmd.name);
                }
            }
            Ok(())
        }
        Commands::Reload {
            completions_dir,
            lang,
            cache_dir,
            dump_dynamic,
        } => build::run_build(completions_dir, lang, cache_dir, dump_dynamic).await,
        Commands::Upgrade => {
            if let Err(e) = upgrade::run_upgrade().await {
                eprintln!("❌ Upgrade failed: {}", e);
                std::process::exit(1);
            }
            Ok(())
        }
        Commands::Init { shell } => {
            let shell_name = shell.unwrap_or_default();
            if shell_name.is_empty() {
                eprintln!("❌ Missing <shell> argument. Usage: sugg init <shell>");
                eprintln!("   Supported shells: bash, zsh, fish, nushell, powershell");
                std::process::exit(1);
            }
            if let Err(e) = init::run_init(&shell_name) {
                eprintln!("❌ Init failed: {}", e);
                std::process::exit(1);
            }
            Ok(())
        }
    }
}
