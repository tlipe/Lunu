mod config;
mod github;
mod package;
mod compat;
mod bridge;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use anyhow::{Result, Context};
use config::Luaurc;
use github::GithubClient;
use package::PackageManager;
use compat::CompatibilityLayer;
use bridge::BridgeManager;

#[derive(Parser)]
#[command(name = "lunu")]
#[command(about = "Lunu Toolchain Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a library from GitHub
    Add {
        /// Search query (e.g., "numpy-luau" or "user/repo")
        query: String,
        
        /// Alias name for local usage (optional, defaults to repo name)
        #[arg(short, long)]
        alias: Option<String>,
    },
    /// Start the Lunu Bridge Server
    Bridge {
        /// Run in background (daemon mode)
        #[arg(short, long)]
        daemon: bool,
    },
    /// Start the Lunu Bridge Server in development mode (foreground)
    Dev,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    // Determine Root (Parent of toolchain or CWD)
    // Assuming toolchain is running from Lunu/toolchain, root is Lunu/.. (Libs folder)
    // But user input implies running in Lunu folder. Let's find .luaurc or use CWD.
    let cwd = std::env::current_dir()?;
    // Search up for .luaurc
    let root = find_root(&cwd).unwrap_or(cwd.clone());
    
    // Don't print "Lunu Root" for bridge/dev command to keep stdout clean
    if !matches!(cli.command, Commands::Bridge { .. } | Commands::Dev) {
        println!("Lunu Root: {:?}", root);
    }

    match cli.command {
        Commands::Bridge { daemon } => {
            let bridge = BridgeManager::new(root);
            bridge.start(daemon)?;
        },
        Commands::Dev => {
            println!("Starting Lunu Dev Server...");
            let bridge = BridgeManager::new(root);
            bridge.start(false)?; // False = Foreground
        },
        Commands::Add { query, alias } => {
            println!("Searching for '{}'...", query);
            
            // 1. Search
            let gh = GithubClient::new(None)?;
            let results = gh.search_packages(&query).await?;
            
            if results.is_empty() {
                println!("No packages found.");
                return Ok(());
            }

            let target = &results[0];
            println!("Found: {}/{} ({})", target.owner, target.name, target.url);

            // 2. Install
            let pm = PackageManager::new(root.clone());
            let install_name = alias.unwrap_or(target.name.clone());
            
            let (path, checksum) = pm.install_package(&target.url, None, &install_name).await?;
            println!("Installed to {:?} (Checksum: {})", path, checksum);

            // 3. Compat
            CompatibilityLayer::ensure_compat(&path).await?;

            // 4. Update Config
            let config_path = root.join(".luaurc");
            let mut config = Luaurc::load(&config_path).await?;
            
            // Lunu specific mapping: mapping modules/name to alias
            // Standard Lune alias format: "alias": "path/to/module"
            // Relative to .luaurc
            let rel_path = pathdiff::diff_paths(&path, &root).unwrap_or(path);
            let rel_path_str = rel_path.to_string_lossy().replace("\\", "/") + "/"; // Add trailing slash for directory modules
            
            config.add_alias(&install_name, &rel_path_str);
            config.save(&config_path).await?;
            
            println!("Updated .luaurc with alias '{}'", install_name);
        }
    }

    Ok(())
}

fn find_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".luaurc").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}
