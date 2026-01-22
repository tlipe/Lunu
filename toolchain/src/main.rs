mod config;
mod github;
mod package;
mod compat;
mod bridge;
mod project;
mod lock;

use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{Result, Context};
use config::Luaurc;
use github::GithubClient;
use package::PackageManager;
use compat::CompatibilityLayer;
use bridge::BridgeManager;
use project::{ProjectConfig, DependencySpec};
use lock::{LockFile, LockEntry};
use fs_extra::dir::{copy as copy_dir, CopyOptions};
use serde_json::Value;
use tokio::fs as async_fs;

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
    /// Build a Luau script into an executable
    Build {
        /// The entry point script (e.g., main.luau)
        script: PathBuf,

        /// Output filename (optional)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Force rebuild ignoring cache
        #[arg(short, long)]
        force: bool,

        /// Open the output after successful build
        #[arg(long)]
        open: bool,

        /// Custom icon path for the executable
        #[arg(long)]
        icon: Option<PathBuf>,

        #[arg(long, num_args = 0..=1, default_missing_value = "true")]
        open_cmd: Option<bool>,
    },
    /// Initialize a Lunu project in the current directory
    Init,
    /// Install dependencies from lunu.toml
    Install,
    /// Remove a dependency
    Remove {
        /// Library name to remove
        lib: String,
    },
    /// Update dependencies
    Update {
        /// Library name to update (optional)
        lib: Option<String>,
    },
    /// List installed dependencies
    List,
    /// Package the project for distribution
    Package,
    /// Manage daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    /// Validate project environment
    Check,
    /// Create a new project
    Create {
        /// Project name (creates a folder with this name)
        name: String,
    },
}

#[derive(Subcommand)]
enum DaemonCommands {
    Status,
    Restart,
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
        Commands::Init => {
            init_project(&root).await?;
        },
        Commands::Create { name } => {
            create_project(&cwd, &name).await?;
        },
        Commands::Install => {
            install_from_config(&root).await?;
        },
        Commands::Remove { lib } => {
            remove_dependency(&root, &lib).await?;
        },
        Commands::Update { lib } => {
            update_dependencies(&root, lib.as_deref()).await?;
        },
        Commands::List => {
            list_dependencies(&root).await?;
        },
        Commands::Package => {
            package_project(&root).await?;
        },
        Commands::Daemon { command } => {
            let lunu_root = resolve_lunu_root(&root);
            match command {
                DaemonCommands::Status => {
                    daemon_status(&lunu_root).await?;
                },
                DaemonCommands::Restart => {
                    daemon_restart(&lunu_root).await?;
                }
            }
        },
        Commands::Check => {
            check_environment(&root).await?;
        },
        Commands::Bridge { daemon } => {
            let bridge = BridgeManager::new(root);
            bridge.start(daemon)?;
        },
        Commands::Dev => {
            println!("Starting Lunu Dev Server...");
            let bridge = BridgeManager::new(root);
            bridge.start(false)?; // False = Foreground
        },
        Commands::Build { script, output, force, open, icon, open_cmd } => {
            // Locate lunu-builder.exe
            let exe_path = std::env::current_exe()?;
            let exe_dir = exe_path.parent().context("Failed to get exe dir")?;
            let builder_exe = exe_dir.join("lunu-builder.exe");

            if !builder_exe.exists() {
                // Try fallback to current directory or standard install path
                // But generally it should be next to lunu.exe
                return Err(anyhow::anyhow!("Builder executable not found at {:?}. Please reinstall Lunu.", builder_exe));
            }

            let mut cmd = std::process::Command::new(builder_exe);
            cmd.arg("build").arg(script);
            
            if let Some(out) = output {
                cmd.arg("--output").arg(out);
            }
            
            if force {
                cmd.arg("--force");
            }

            if open {
                cmd.arg("--open");
            }

            if let Some(icon_path) = icon {
                cmd.arg("--icon").arg(icon_path);
            }

            if let Some(value) = open_cmd {
                cmd.arg("--open-cmd");
                if !value {
                    cmd.arg("false");
                }
            }

            // Inherit stdio
            let mut child = cmd.spawn()?;
            let status = child.wait()?;
            
            if !status.success() {
                return Err(anyhow::anyhow!("Build failed with exit code: {:?}", status.code()));
            }
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

            let config_path = project_config_path(&root);
            let mut proj = load_or_init_project(&root, &config_path).await?;
            let mut spec = DependencySpec::default();
            spec.url = Some(target.url.clone());
            spec.path = Some(rel_path_str.trim_end_matches('/').to_string());
            proj.add_dependency(&install_name, spec);
            proj.save(&config_path).await?;

            let lock_path = lock_path(&root);
            let mut lock = LockFile::load(&lock_path).await?;
            lock.set(&install_name, LockEntry {
                url: Some(target.url.clone()),
                version: None,
                path: Some(rel_path_str.trim_end_matches('/').to_string()),
                checksum,
                installed_at: current_timestamp(),
            });
            lock.save(&lock_path).await?;
        }
    }

    Ok(())
}

fn project_config_path(root: &Path) -> PathBuf {
    root.join("lunu.toml")
}

fn lock_path(root: &Path) -> PathBuf {
    root.join("lunu.lock")
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn resolve_lunu_root(root: &Path) -> PathBuf {
    if root.ends_with("Lunu") {
        root.to_path_buf()
    } else {
        root.join("Lunu")
    }
}

fn project_name_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("lunu-project")
        .to_string()
}

fn scan_modules(root: &Path) -> BTreeMap<String, DependencySpec> {
    let mut deps = BTreeMap::new();
    let modules_dir = root.join("modules");
    if let Ok(read_dir) = std::fs::read_dir(modules_dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    let mut spec = DependencySpec::default();
                    spec.path = Some(format!("modules/{}", name));
                    deps.insert(name.to_string(), spec);
                }
            }
        }
    }
    deps
}

async fn load_or_init_project(root: &Path, config_path: &Path) -> Result<ProjectConfig> {
    if config_path.exists() {
        ProjectConfig::load(config_path).await
    } else {
        let name = project_name_from_root(root);
        let cfg = ProjectConfig::new(&name);
        cfg.save(config_path).await?;
        Ok(cfg)
    }
}

async fn ensure_project_files(root: &Path) -> Result<()> {
    let src_dir = root.join("src");
    let modules_dir = root.join("modules");
    if !src_dir.exists() {
        async_fs::create_dir_all(&src_dir).await?;
    }
    if !modules_dir.exists() {
        async_fs::create_dir_all(&modules_dir).await?;
    }
    let main_path = src_dir.join("main.luau");
    if !main_path.exists() {
        async_fs::write(&main_path, "print(\"Hello from Lunu\")\n").await?;
    }
    Ok(())
}

async fn update_luaurc(root: &Path, deps: &BTreeMap<String, DependencySpec>) -> Result<()> {
    let config_path = root.join(".luaurc");
    let mut luaurc = Luaurc::load(&config_path).await?;
    let lunu_alias = if root.join("Lunu").exists() {
        "Lunu/".to_string()
    } else if root.parent().map(|p| p.join("Lunu").exists()).unwrap_or(false) {
        "../Lunu/".to_string()
    } else {
        "Lunu/".to_string()
    };
    luaurc.add_alias("lunu", &lunu_alias);
    for (name, spec) in deps {
        if let Some(path) = &spec.path {
            let rel_path = path.trim_start_matches("./");
            let rel_path_str = rel_path.replace("\\", "/") + "/";
            luaurc.add_alias(name, &rel_path_str);
        }
    }
    luaurc.save(&config_path).await?;
    Ok(())
}

async fn init_project(root: &Path) -> Result<()> {
    ensure_project_files(root).await?;
    let config_path = project_config_path(root);
    let mut cfg = load_or_init_project(root, &config_path).await?;

    let discovered = scan_modules(root);
    for (name, spec) in discovered {
        cfg.add_dependency(&name, spec);
    }
    cfg.save(&config_path).await?;

    update_luaurc(root, &cfg.dependencies).await?;

    let lock_path = lock_path(root);
    let mut lock = LockFile::load(&lock_path).await?;
    let pm = PackageManager::new(root.to_path_buf());
    for (name, spec) in &cfg.dependencies {
        if let Some(path) = &spec.path {
            let full_path = root.join(path);
            if full_path.exists() {
                let checksum = pm.calculate_dir_checksum(&full_path).await?;
                lock.set(name, LockEntry {
                    url: spec.url.clone(),
                    version: spec.version.clone(),
                    path: Some(path.clone()),
                    checksum,
                    installed_at: current_timestamp(),
                });
            }
        }
    }
    lock.save(&lock_path).await?;

    println!("Project initialized at {:?}", root);
    Ok(())
}

async fn create_project(cwd: &Path, name: &str) -> Result<()> {
    let project_dir = cwd.join(name);
    if project_dir.exists() {
        return Err(anyhow::anyhow!("Directory '{}' already exists", name));
    }
    async_fs::create_dir_all(&project_dir).await?;
    init_project(&project_dir).await?;
    Ok(())
}

async fn install_from_config(root: &Path) -> Result<()> {
    let config_path = project_config_path(root);
    if !config_path.exists() {
        return Err(anyhow::anyhow!("lunu.toml not found. Run 'lunu init' first."));
    }
    let cfg = ProjectConfig::load(&config_path).await?;
    let mut lock = LockFile::load(&lock_path(root)).await?;
    let pm = PackageManager::new(root.to_path_buf());

    if cfg.dependencies.is_empty() {
        println!("No dependencies listed in lunu.toml.");
        return Ok(());
    }

    for (name, spec) in &cfg.dependencies {
        if let Some(url) = &spec.url {
            let (path, checksum) = pm.install_package(url, spec.version.as_deref(), name).await?;
            CompatibilityLayer::ensure_compat(&path).await?;

            let rel_path = pathdiff::diff_paths(&path, root).unwrap_or(path);
            let rel_path_str = rel_path.to_string_lossy().replace("\\", "/");
            lock.set(name, LockEntry {
                url: Some(url.clone()),
                version: spec.version.clone(),
                path: Some(rel_path_str.clone()),
                checksum,
                installed_at: current_timestamp(),
            });
        } else if let Some(path) = &spec.path {
            let full_path = root.join(path);
            if full_path.exists() {
                let checksum = pm.calculate_dir_checksum(&full_path).await?;
                lock.set(name, LockEntry {
                    url: None,
                    version: spec.version.clone(),
                    path: Some(path.clone()),
                    checksum,
                    installed_at: current_timestamp(),
                });
            }
        }
    }

    update_luaurc(root, &cfg.dependencies).await?;
    lock.save(&lock_path(root)).await?;
    println!("Dependencies installed successfully.");
    Ok(())
}

async fn remove_dependency(root: &Path, lib: &str) -> Result<()> {
    let config_path = project_config_path(root);
    if !config_path.exists() {
        return Err(anyhow::anyhow!("lunu.toml not found. Run 'lunu init' first."));
    }
    let mut cfg = ProjectConfig::load(&config_path).await?;
    cfg.remove_dependency(lib);
    cfg.save(&config_path).await?;

    let mut lock = LockFile::load(&lock_path(root)).await?;
    lock.remove(lib);
    lock.save(&lock_path(root)).await?;

    let pm = PackageManager::new(root.to_path_buf());
    pm.remove_package(lib).await?;

    let luaurc_path = root.join(".luaurc");
    let mut luaurc = Luaurc::load(&luaurc_path).await?;
    luaurc.remove_alias(lib);
    luaurc.save(&luaurc_path).await?;

    println!("Removed dependency '{}'.", lib);
    Ok(())
}

async fn update_dependencies(root: &Path, lib: Option<&str>) -> Result<()> {
    let config_path = project_config_path(root);
    if !config_path.exists() {
        return Err(anyhow::anyhow!("lunu.toml not found. Run 'lunu init' first."));
    }
    let cfg = ProjectConfig::load(&config_path).await?;
    let mut lock = LockFile::load(&lock_path(root)).await?;
    let pm = PackageManager::new(root.to_path_buf());

    let targets: Vec<(&String, &DependencySpec)> = cfg.dependencies.iter().collect();
    for (name, spec) in targets {
        if let Some(filter) = lib {
            if name != filter {
                continue;
            }
        }

        if let Some(url) = &spec.url {
            let (path, checksum) = pm.install_package(url, spec.version.as_deref(), name).await?;
            CompatibilityLayer::ensure_compat(&path).await?;
            let rel_path = pathdiff::diff_paths(&path, root).unwrap_or(path);
            let rel_path_str = rel_path.to_string_lossy().replace("\\", "/");
            lock.set(name, LockEntry {
                url: Some(url.clone()),
                version: spec.version.clone(),
                path: Some(rel_path_str),
                checksum,
                installed_at: current_timestamp(),
            });
        }
    }

    lock.save(&lock_path(root)).await?;
    println!("Dependencies updated.");
    Ok(())
}

async fn list_dependencies(root: &Path) -> Result<()> {
    let lock = LockFile::load(&lock_path(root)).await?;
    if lock.dependencies.is_empty() {
        println!("No dependencies installed.");
        return Ok(());
    }

    for (name, entry) in lock.dependencies {
        let source = entry.url.or(entry.path).unwrap_or_else(|| "unknown".to_string());
        let version = entry.version.unwrap_or_else(|| "latest".to_string());
        println!("{} | {} | {}", name, version, source);
    }
    Ok(())
}

async fn package_project(root: &Path) -> Result<()> {
    let config_path = project_config_path(root);
    if !config_path.exists() {
        return Err(anyhow::anyhow!("lunu.toml not found. Run 'lunu init' first."));
    }
    let cfg = ProjectConfig::load(&config_path).await?;
    let dist_dir = root.join("dist");
    if dist_dir.exists() {
        async_fs::remove_dir_all(&dist_dir).await?;
    }
    async_fs::create_dir_all(&dist_dir).await?;

    let entry_path = PathBuf::from(&cfg.project.entry);
    let stem = entry_path.file_stem().and_then(|s| s.to_str()).unwrap_or("main");
    let exe_path = root.join(format!("{}.exe", stem));
    if exe_path.exists() {
        async_fs::copy(&exe_path, dist_dir.join(exe_path.file_name().unwrap())).await?;
    } else {
        println!("Warning: executable not found at {:?}", exe_path);
    }

    let modules_dir = root.join("modules");
    if modules_dir.exists() {
        let mut options = CopyOptions::new();
        options.copy_inside = true;
        copy_dir(&modules_dir, dist_dir.join("modules"), &options)?;
    }

    let assets_dir = root.join("assets");
    if assets_dir.exists() {
        let mut options = CopyOptions::new();
        options.copy_inside = true;
        copy_dir(&assets_dir, dist_dir.join("assets"), &options)?;
    }

    let lock_path = lock_path(root);
    if lock_path.exists() {
        async_fs::copy(&lock_path, dist_dir.join("lunu.lock")).await?;
    }
    async_fs::copy(&config_path, dist_dir.join("lunu.toml")).await?;

    println!("Package created at {:?}", dist_dir);
    Ok(())
}

async fn daemon_status(lunu_root: &Path) -> Result<()> {
    let port = read_server_port(lunu_root)?;
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::new();
    let res = client.get(url).send().await;
    match res {
        Ok(r) if r.status().is_success() => {
            println!("Daemon status: running (port {})", port);
        },
        _ => {
            println!("Daemon status: stopped (port {})", port);
        }
    }
    Ok(())
}

async fn daemon_restart(lunu_root: &Path) -> Result<()> {
    let port = read_server_port(lunu_root)?;
    let api_key = read_api_key(lunu_root).unwrap_or_default();
    let url = format!("http://127.0.0.1:{}/api/v1/system/shutdown", port);
    let client = reqwest::Client::new();
    let _ = client
        .post(url)
        .header("X-LUNU-KEY", api_key)
        .send()
        .await;

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let bridge = BridgeManager::new(lunu_root.to_path_buf());
    bridge.start(true)?;
    println!("Daemon restarted.");
    Ok(())
}

fn read_server_port(lunu_root: &Path) -> Result<u16> {
    let settings_path = lunu_root.join("config").join("settings.json");
    let content = std::fs::read_to_string(&settings_path)
        .with_context(|| format!("Failed to read {:?}", settings_path))?;
    let value: Value = serde_json::from_str(&content)?;
    let port = value["server"]["http_port"].as_u64().unwrap_or(8000) as u16;
    Ok(port)
}

fn read_api_key(lunu_root: &Path) -> Result<String> {
    let secrets_path = lunu_root.join("config").join(".secrets.json");
    if !secrets_path.exists() {
        return Ok(String::new());
    }
    let content = std::fs::read_to_string(&secrets_path)
        .with_context(|| format!("Failed to read {:?}", secrets_path))?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(value["api_key"].as_str().unwrap_or("").to_string())
}

async fn check_environment(root: &Path) -> Result<()> {
    let lunu_root = resolve_lunu_root(root);
    let config_path = project_config_path(root);
    let lock_path = lock_path(root);
    let modules_dir = root.join("modules");
    let src_main = root.join("src").join("main.luau");
    let builder_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lunu-builder.exe")))
        .unwrap_or_else(|| root.join("bin").join("lunu-builder.exe"));

    println!("Environment check:");
    println!("- Lunu directory: {}", lunu_root.exists());
    println!("- Project config (lunu.toml): {}", config_path.exists());
    println!("- Lock file (lunu.lock): {}", lock_path.exists());
    println!("- Modules directory: {}", modules_dir.exists());
    println!("- Entry file: {}", src_main.exists());
    println!("- Builder executable: {}", builder_exe.exists());
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn project_name_from_root_works() {
        let dir = tempdir().unwrap();
        let name = project_name_from_root(dir.path());
        assert!(!name.is_empty());
    }

    #[test]
    fn scan_modules_detects_dirs() {
        let dir = tempdir().unwrap();
        let modules_dir = dir.path().join("modules");
        std::fs::create_dir_all(modules_dir.join("demo")).unwrap();
        let deps = scan_modules(dir.path());
        assert!(deps.contains_key("demo"));
    }
}
