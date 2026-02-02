mod config;
mod github;
mod package;
mod compat;
mod project;
mod lock;

use clap::{Parser, Subcommand, ValueEnum};
use lunu_cli::bridge_server;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf, Component};
use std::time::{SystemTime, UNIX_EPOCH};
use std::process::Command;
use std::fs::{self, File};
use reqwest::header::USER_AGENT;
use anyhow::{Result, Context};
use config::Luaurc;
use github::GithubClient;
use package::PackageManager;
use compat::CompatibilityLayer;
use project::{ProjectConfig, DependencySpec, RuntimeConfig, BuildConfig};
use lock::{LockFile, LockEntry};
use fs_extra::dir::{copy as copy_dir, CopyOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs as async_fs;
use flate2::read::GzDecoder;
use tar::Archive;

use std::io::{self, Write};
#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winapi::um::consoleapi::GetConsoleMode;
#[cfg(windows)]
use winapi::um::processenv::GetStdHandle;
#[cfg(windows)]
use winapi::um::winbase::STD_INPUT_HANDLE;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;


#[derive(Parser)]
#[command(name = "lunu")]
#[command(version = "0.1.1")]
#[command(about = "Lunu Toolchain Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
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
    /// Validate project environment
    Check,
    /// Create a new project
    Create {
        /// Project name (creates a folder with this name)
        name: String,
    },
    /// Upgrade Lunu to the latest version
    Upgrade,
    /// Uninstall Lunu from the system
    Uninstall,
    /// Scaffold a new project with a template
    Scaffold {
        /// Project name (creates a folder with this name)
        name: String,
        /// Template type
        #[arg(short, long, value_enum, default_value_t = TemplateKind::App)]
        template: TemplateKind,
    },
    /// Create a bridge module scaffold
    Module {
        /// Module name (creates modules/<name>)
        name: String,
        /// Worker language
        #[arg(short, long, value_enum, default_value_t = ModuleLang::Python)]
        lang: ModuleLang,
    },
    /// Profile a Luau script using the Lune runtime
    Profile {
        /// The entry point script (e.g., src/main.luau)
        script: PathBuf,
        /// Number of runs
        #[arg(short, long, default_value_t = 1)]
        runs: u32,
    },
    /// Run a script using the project runtime (Lute or Lune) resolved from config/env
    Run {
        /// The entry point script (e.g., src/main.luau)
        script: PathBuf,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Run project tests (*.test.luau, *.spec.luau)
    Test {
        /// Specific test file to run (optional)
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
    /// Manage a specific runtime
    Runtime {
        #[arg(value_enum)]
        runtime: RuntimeTarget,
        /// Update the runtime from the official GitHub release
        #[arg(long)]
        update: bool,
    },
    /// Manage all runtimes
    Runtimes {
        /// Update all runtimes from official GitHub releases
        #[arg(long)]
        update: bool,
    },
    /// Clean internal cache
    Clean,
}

#[derive(ValueEnum, Clone)]
enum TemplateKind {
    App,
    Game,
}

#[derive(ValueEnum, Clone)]
enum ModuleLang {
    Python,
    Node,
    Rust,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
enum RuntimeTarget {
    Lute,
    Lune,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimeKind {
    Lute,
    Lune,
}



const LUTE_REPO: &str = "luau-lang/lute";
const LUNE_REPO: &str = "lune-org/lune";
const LUTE_EMBEDDED_VERSION: &str = "0.1.0";
const UPDATE_CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60;

struct ToolchainDetection {
    c_compiler: Option<PathBuf>,
    cpp_compiler: Option<PathBuf>,
    toolchain: Option<String>,
}

fn stdin_is_interactive() -> bool {
    #[cfg(windows)]
    {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE);
            if handle.is_null() {
                return false;
            }
            let mut mode = 0u32;
            GetConsoleMode(handle, &mut mode) != 0
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn runtime_from_env() -> Option<RuntimeKind> {
    for key in ["LUNU_RUNTIME", "LUNU_INIT_RUNTIME"] {
        if let Ok(value) = std::env::var(key) {
            let v = value.trim().to_lowercase();
            if v == "lute" || v == "c++" || v == "cpp" {
                return Some(RuntimeKind::Lute);
            }
            if v == "lune" || v == "rust" {
                return Some(RuntimeKind::Lune);
            }
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize, Clone)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RuntimeMeta {
    version: String,
    source: String,
}

struct RuntimeUpdate {
    version: String,
    url: String,
}

#[derive(serde::Deserialize)]
struct GithubRepoInfo {
    default_branch: String,
}

fn runtime_target_from_kind(kind: RuntimeKind) -> RuntimeTarget {
    match kind {
        RuntimeKind::Lute => RuntimeTarget::Lute,
        RuntimeKind::Lune => RuntimeTarget::Lune,
    }
}

fn runtime_repo(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Lute => LUTE_REPO,
        RuntimeTarget::Lune => LUNE_REPO,
    }
}

fn runtime_name(target: RuntimeTarget) -> &'static str {
    match target {
        RuntimeTarget::Lute => "lute",
        RuntimeTarget::Lune => "lune",
    }
}

fn runtime_bin_filename(target: RuntimeTarget) -> String {
    if cfg!(windows) {
        format!("{}.exe", runtime_name(target))
    } else {
        runtime_name(target).to_string()
    }
}

fn executable_extension() -> Option<&'static str> {
    if cfg!(windows) {
        Some("exe")
    } else {
        None
    }
}

fn lunu_bin_filename() -> String {
    if cfg!(windows) {
        "lunu.exe".to_string()
    } else {
        "lunu".to_string()
    }
}

fn old_exe_path(current_exe: &Path) -> PathBuf {
    if cfg!(windows) {
        current_exe.with_extension("exe.old")
    } else {
        current_exe.with_extension("old")
    }
}

fn builder_bin_filename() -> String {
    if cfg!(windows) {
        "lunu-builder.exe".to_string()
    } else {
        "lunu-builder".to_string()
    }
}

fn platform_os_keys() -> Vec<&'static str> {
    match std::env::consts::OS {
        "windows" => vec!["windows", "win"],
        "macos" => vec!["macos", "darwin", "osx"],
        "linux" => vec!["linux"],
        other => vec![other],
    }
}

fn platform_arch_keys() -> Vec<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => vec!["x86_64", "amd64", "x64"],
        "aarch64" => vec!["aarch64", "arm64"],
        "arm" => vec!["armv7", "arm"],
        other => vec![other],
    }
}

fn asset_matches_platform(name: &str, require_arch: bool) -> bool {
    let os_keys = platform_os_keys();
    let arch_keys = platform_arch_keys();
    let has_os = os_keys.iter().any(|k| name.contains(k));
    if !has_os {
        return false;
    }
    if require_arch {
        let has_arch = arch_keys.iter().any(|k| name.contains(k));
        if !has_arch {
            return false;
        }
    }
    true
}

fn asset_extension_supported(name: &str) -> bool {
    if name.ends_with(".zip") || name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return true;
    }
    if cfg!(windows) && name.ends_with(".exe") {
        return true;
    }
    let plain = !name.contains('.') && !name.ends_with('/') && !name.ends_with('\\');
    if !cfg!(windows) && plain {
        return true;
    }
    false
}

fn ensure_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn matches_candidate_name(file_name: &str, candidates: &[String]) -> bool {
    candidates
        .iter()
        .any(|candidate| file_name.eq_ignore_ascii_case(candidate))
}

fn extract_binary_from_zip(bytes: &[u8], candidates: &[String]) -> Result<Vec<u8>> {
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.name().ends_with('/') {
            continue;
        }
        let name = file.name().to_string();
        let file_name = Path::new(&name).file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches_candidate_name(file_name, candidates) {
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut content)?;
            return Ok(content);
        }
    }
    Err(anyhow::anyhow!("Binary not found in zip archive"))
}

fn extract_binary_from_tar_gz(bytes: &[u8], candidates: &[String]) -> Result<Vec<u8>> {
    let reader = std::io::Cursor::new(bytes);
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let entry_path = entry.path()?;
        let file_name = entry_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches_candidate_name(file_name, candidates) {
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content)?;
            return Ok(content);
        }
    }
    Err(anyhow::anyhow!("Binary not found in tar archive"))
}

fn runtime_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("lunu")
        .join("runtimes")
}

fn runtime_cache_bin(target: RuntimeTarget) -> PathBuf {
    runtime_cache_dir().join(runtime_bin_filename(target))
}

fn runtime_lib_root(target: RuntimeTarget) -> PathBuf {
    runtime_cache_dir().join(runtime_name(target))
}

fn lute_sources_root() -> PathBuf {
    runtime_cache_dir().join("lute-src")
}

fn runtime_available(root: &Path, target: RuntimeTarget) -> bool {
    let local = root.join("bin").join(runtime_bin_filename(target));
    if local.exists() {
        return true;
    }
    let env_key = match target {
        RuntimeTarget::Lute => "LUTE_PATH",
        RuntimeTarget::Lune => "LUNE_PATH",
    };
    if let Ok(path) = std::env::var(env_key) {
        if PathBuf::from(path).exists() {
            return true;
        }
    }
    let cached = runtime_cache_bin(target);
    if cached.exists() {
        return true;
    }
    find_in_path(&runtime_bin_filename(target)).is_some()
}

async fn ensure_runtime_available(root: &Path, target: RuntimeTarget) -> Result<()> {
    if runtime_available(root, target) {
        return Ok(());
    }
    if let Err(err) = update_runtime(target).await {
        if target == RuntimeTarget::Lute && ensure_embedded_lute().is_some() {
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

fn runtime_meta_path(target: RuntimeTarget) -> PathBuf {
    runtime_cache_dir().join(format!("{}.json", runtime_name(target)))
}

fn read_runtime_meta(target: RuntimeTarget) -> Option<RuntimeMeta> {
    let path = runtime_meta_path(target);
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_runtime_meta(target: RuntimeTarget, meta: &RuntimeMeta) -> Result<()> {
    let path = runtime_meta_path(target);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string(meta)?;
    std::fs::write(path, content)?;
    Ok(())
}

#[derive(Serialize, Deserialize, Default)]
struct UpdateCheckCache {
    last_check: BTreeMap<String, u64>,
}

fn update_check_cache_path() -> PathBuf {
    runtime_cache_dir().join("update-check.json")
}

fn read_update_check_cache() -> UpdateCheckCache {
    let path = update_check_cache_path();
    if let Ok(content) = std::fs::read_to_string(path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        UpdateCheckCache::default()
    }
}

fn write_update_check_cache(cache: &UpdateCheckCache) -> Result<()> {
    let path = update_check_cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string(cache)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn should_check_update(cache: &UpdateCheckCache, target: RuntimeTarget) -> bool {
    let key = runtime_name(target);
    match cache.last_check.get(key) {
        Some(value) => current_timestamp().saturating_sub(*value) >= UPDATE_CHECK_INTERVAL_SECS,
        None => true,
    }
}

fn record_update_check(cache: &mut UpdateCheckCache, target: RuntimeTarget) {
    cache
        .last_check
        .insert(runtime_name(target).to_string(), current_timestamp());
}

fn ensure_embedded_lute() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let target = RuntimeTarget::Lute;
    let path = runtime_cache_bin(target);
    if path.exists() {
        return Some(path);
    }
    if let Some(bytes) = embedded_lute_bytes() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut f) = File::create(&path) {
            let _ = f.write_all(bytes);
        }
        if path.exists() {
            if read_runtime_meta(target).is_none() {
                let _ = write_runtime_meta(
                    target,
                    &RuntimeMeta {
                        version: LUTE_EMBEDDED_VERSION.to_string(),
                        source: "embedded".to_string(),
                    },
                );
            }
            return Some(path);
        }
    }
    None
}

async fn fetch_latest_release(target: RuntimeTarget) -> Result<GithubRelease> {
    let repo = runtime_repo(target);
    // Use /releases instead of /releases/latest to catch pre-releases (nightly)
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(USER_AGENT, "Lunu-CLI")
        .send()
        .await?
        .error_for_status()?;
    
    // The API returns an array of releases. We want the first one.
    let releases: Vec<GithubRelease> = resp.json().await?;
    releases.into_iter().next().ok_or_else(|| anyhow::anyhow!("No releases found for {}", repo))
}

fn pick_runtime_asset(release: &GithubRelease, target: RuntimeTarget) -> Option<GithubAsset> {
    let name = runtime_name(target);
    let mut candidates: Vec<GithubAsset> = release
        .assets
        .iter()
        .filter(|a| {
            let n = a.name.to_lowercase();
            n.contains(name) && asset_extension_supported(&n)
        })
        .cloned()
        .collect();

    candidates.sort_by_key(|a| a.name.to_lowercase());

    for require_arch in [true, false] {
        if let Some(asset) = candidates.iter().find(|a| {
            let n = a.name.to_lowercase();
            asset_matches_platform(&n, require_arch)
        }) {
            return Some(asset.clone());
        }
    }
    None
}

async fn find_runtime_update(target: RuntimeTarget) -> Result<Option<RuntimeUpdate>> {
    let latest = fetch_latest_release(target).await?;
    let current = read_runtime_meta(target).map(|m| m.version);
    if let Some(ref current) = current {
        if current == &latest.tag_name {
            return Ok(None);
        }
    }
    let asset = pick_runtime_asset(&latest, target)
        .ok_or_else(|| anyhow::anyhow!("No compatible runtime asset found in latest {} release", runtime_name(target)))?;
    Ok(Some(RuntimeUpdate {
        version: latest.tag_name,
        url: asset.browser_download_url,
    }))
}

async fn download_runtime(target: RuntimeTarget, update: &RuntimeUpdate) -> Result<PathBuf> {
    let url = &update.url;
    println!("Downloading {} from {}...", runtime_name(target), url);
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(USER_AGENT, "Lunu-CLI")
        .send()
        .await?
        .error_for_status()?;
    let bytes = resp.bytes().await?;
    
    let path = runtime_cache_bin(target);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lib_root = runtime_lib_root(target);
    let url_lower = url.to_lowercase();
    let runtime_filename = runtime_bin_filename(target);
    let runtime_base = runtime_name(target).to_string();

    if url_lower.ends_with(".zip") {
        let reader = std::io::Cursor::new(bytes);
        let mut zip = zip::ZipArchive::new(reader)?;
        let mut runtime_bytes: Option<Vec<u8>> = None;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i)?;
            let name = file.name().to_string();
            if name.ends_with('/') {
                continue;
            }
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut content)?;
            let entry_path = Path::new(&name);
            let file_name = entry_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let matches = file_name.eq_ignore_ascii_case(&runtime_filename) || file_name.eq_ignore_ascii_case(&runtime_base);
            if matches && runtime_bytes.is_none() {
                runtime_bytes = Some(content.clone());
            }
            let mut rel = PathBuf::new();
            for comp in entry_path.components() {
                if let Component::Normal(c) = comp {
                    rel.push(c);
                }
            }
            if rel.as_os_str().is_empty() {
                continue;
            }
            let out_path = lib_root.join(rel);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::fs::write(&out_path, &content)?;
        }
        let content = runtime_bytes.ok_or(anyhow::anyhow!("Runtime binary not found in zip"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        ensure_executable(&path)?;
    } else if url_lower.ends_with(".tar.gz") || url_lower.ends_with(".tgz") {
        let reader = std::io::Cursor::new(bytes);
        let decoder = GzDecoder::new(reader);
        let mut archive = Archive::new(decoder);
        let mut runtime_bytes: Option<Vec<u8>> = None;
        let entries = archive.entries()?;
        for entry in entries {
            let mut entry = entry?;
            if entry.header().entry_type().is_dir() {
                continue;
            }
            let entry_path = entry.path()?.to_path_buf();
            let file_name = entry_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let mut content = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut content)?;
            let matches = file_name.eq_ignore_ascii_case(&runtime_filename) || file_name.eq_ignore_ascii_case(&runtime_base);
            if matches && runtime_bytes.is_none() {
                runtime_bytes = Some(content.clone());
            }
            let mut rel = PathBuf::new();
            for comp in entry_path.components() {
                if let Component::Normal(c) = comp {
                    rel.push(c);
                }
            }
            if rel.as_os_str().is_empty() {
                continue;
            }
            let out_path = lib_root.join(rel);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::fs::write(&out_path, &content)?;
        }
        let content = runtime_bytes.ok_or(anyhow::anyhow!("Runtime binary not found in tarball"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, content)?;
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        ensure_executable(&path)?;
    } else {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        if path.exists() {
             let _ = std::fs::remove_file(&path);
        }
        std::fs::rename(&tmp, &path)?;
        ensure_executable(&path)?;
    }

    write_runtime_meta(
        target,
        &RuntimeMeta {
            version: update.version.clone(),
            source: "github".to_string(),
        },
    )?;
    Ok(path)
}

async fn update_runtime(target: RuntimeTarget) -> Result<()> {
    match find_runtime_update(target).await? {
        Some(update) => {
            let path = download_runtime(target, &update).await?;
            println!("Updated {} runtime to {} at {:?}", runtime_name(target), update.version, path);
        }
        None => {
            println!("{} runtime is up to date", runtime_name(target));
        }
    }
    Ok(())
}

async fn fetch_repo_default_branch(repo: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{}", repo);
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(USER_AGENT, "Lunu-CLI")
        .send()
        .await?
        .error_for_status()?;
    let info: GithubRepoInfo = resp.json().await?;
    Ok(info.default_branch)
}

async fn download_repo_zip(repo: &str, branch: &str) -> Result<Vec<u8>> {
    let url = format!("https://codeload.github.com/{}/zip/refs/heads/{}", repo, branch);
    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .header(USER_AGENT, "Lunu-CLI")
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

async fn ensure_lute_sources() -> Result<PathBuf> {
    let root = lute_sources_root();
    let std_dir = root.join("std");
    let lute_dir = root.join("lute");
    if std_dir.exists() && lute_dir.exists() {
        return Ok(root);
    }
    if root.exists() {
        let _ = fs::remove_dir_all(&root);
    }
    let branch = fetch_repo_default_branch(LUTE_REPO).await.unwrap_or_else(|_| "main".to_string());
    let mut bytes = download_repo_zip(LUTE_REPO, &branch).await;
    if bytes.is_err() {
        for fallback in ["main", "master"] {
            if fallback != branch {
                if let Ok(value) = download_repo_zip(LUTE_REPO, fallback).await {
                    bytes = Ok(value);
                    break;
                }
            }
        }
    }
    let bytes = bytes?;
    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        let mut parts = name.splitn(2, '/');
        let _prefix = parts.next();
        let relative = match parts.next() {
            Some(v) => v,
            None => continue,
        };
        if !(relative.starts_with("std/") || relative.starts_with("lute/") || relative.starts_with("batteries/")) {
            continue;
        }
        let out_path = root.join(relative);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut content)?;
        std::fs::write(&out_path, &content)?;
    }
    Ok(root)
}

async fn maybe_prompt_update(target: RuntimeTarget) -> Result<()> {
    if !stdin_is_interactive() {
        return Ok(());
    }
    let mut cache = read_update_check_cache();
    if !should_check_update(&cache, target) {
        return Ok(());
    }
    record_update_check(&mut cache, target);
    let _ = write_update_check_cache(&cache);
    tokio::spawn(async move {
        let update = match find_runtime_update(target).await {
            Ok(value) => value,
            Err(err) => {
                println!("Runtime update check failed for {}: {}", runtime_name(target), err);
                return;
            }
        };
        if let Some(update) = update {
            println!(
                "Update {} runtime available: {}. Run 'lunu runtime {} --update' to install.",
                runtime_name(target),
                update.version,
                runtime_name(target)
            );
        }
    });
    Ok(())
}

fn select_runtime() -> Result<RuntimeKind> {
    if let Some(runtime) = runtime_from_env() {
        return Ok(runtime);
    }
    if !stdin_is_interactive() {
        return Ok(RuntimeKind::Lune);
    }

    println!("Select a runtime for this project:");
    println!("1) C++ - Lute");
    println!("   Security: full system access, no sandboxing, maximum flexibility");
    println!("   Performance: highest, direct native execution and module integration");
    println!("   Libraries: @lute and @std are available by default");
    println!("2) Rust - Lune");
    println!("   Security: sandboxed and safer defaults with bridge boundaries");
    println!("   Performance: great for tooling, bridge calls add overhead");
    println!("   Libraries: @lune is used for bridge-driven integrations");
    print!("Choose [1-2] (default 2): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice = input.trim().to_lowercase();
    if choice.is_empty() || choice == "2" || choice == "lune" {
        return Ok(RuntimeKind::Lune);
    }
    if choice == "1" || choice == "lute" {
        return Ok(RuntimeKind::Lute);
    }

    loop {
        print!("Please enter 1 (Lute) or 2 (Lune): ");
        io::stdout().flush()?;
        input.clear();
        io::stdin().read_line(&mut input)?;
        let value = input.trim().to_lowercase();
        if value == "1" || value == "lute" {
            return Ok(RuntimeKind::Lute);
        }
        if value == "2" || value == "lune" {
            return Ok(RuntimeKind::Lune);
        }
    }
}

fn runtime_config_for(runtime: RuntimeKind) -> RuntimeConfig {
    match runtime {
        RuntimeKind::Lute => RuntimeConfig {
            name: "lute".to_string(),
            security: "Full system access, no sandboxing, maximum flexibility".to_string(),
            performance: "Highest performance with direct native execution".to_string(),
            notes: "Use @lute and @std, build native modules directly (C/C++/Rust)".to_string(),
        },
        RuntimeKind::Lune => RuntimeConfig {
            name: "lune".to_string(),
            security: "Sandboxed defaults with bridge isolation".to_string(),
            performance: "Great for tooling; bridge calls add overhead".to_string(),
            notes: "Bridge-based integration for external languages".to_string(),
        },
    }
}

fn runtime_kind_from_config(cfg: &ProjectConfig) -> RuntimeKind {
    match cfg.runtime.as_ref().map(|r| r.name.as_str()) {
        Some("lute") => RuntimeKind::Lute,
        _ => RuntimeKind::Lune,
    }
}

async fn resolve_runtime_for_root(root: &Path) -> Result<RuntimeKind> {
    if let Some(runtime) = runtime_from_env() {
        return Ok(runtime);
    }
    let config_path = project_config_path(root);
    if config_path.exists() {
        if let Ok(cfg) = ProjectConfig::load(&config_path).await {
            return Ok(runtime_kind_from_config(&cfg));
        }
    }
    Ok(RuntimeKind::Lune)
}

fn build_config_for(runtime: RuntimeKind, toolchain: Option<ToolchainDetection>) -> BuildConfig {
    match runtime {
        RuntimeKind::Lute => {
            let toolchain = toolchain.unwrap_or(ToolchainDetection {
                c_compiler: None,
                cpp_compiler: None,
                toolchain: None,
            });
            BuildConfig {
                kind: "native".to_string(),
                link: "direct".to_string(),
                modules: "native".to_string(),
                module_languages: vec!["c".to_string(), "cpp".to_string(), "rust".to_string()],
                features: vec![
                    "std".to_string(),
                    "lute".to_string(),
                    "direct-build".to_string(),
                ],
                c_compiler: toolchain
                    .c_compiler
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                cpp_compiler: toolchain
                    .cpp_compiler
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                toolchain: toolchain.toolchain,
            }
        }
        RuntimeKind::Lune => BuildConfig {
            kind: "bridge".to_string(),
            link: "http-bridge".to_string(),
            modules: "bridge".to_string(),
            module_languages: vec![],
            features: vec![
                "sandboxed".to_string(),
                "bridge-workers".to_string(),
            ],
            c_compiler: None,
            cpp_compiler: None,
            toolchain: None,
        },
    }
}

fn detect_cpp_toolchain() -> ToolchainDetection {
    #[cfg(windows)]
    let cpp_candidates = [
        ("cl.exe", "msvc"),
        ("clang-cl.exe", "clang-cl"),
        ("clang++.exe", "clang"),
        ("g++.exe", "gcc"),
    ];
    #[cfg(not(windows))]
    let cpp_candidates = [
        ("c++", "cc"),
        ("clang++", "clang"),
        ("g++", "gcc"),
    ];
    #[cfg(windows)]
    let c_candidates = [
        ("cl.exe", "msvc"),
        ("clang-cl.exe", "clang-cl"),
        ("clang.exe", "clang"),
        ("gcc.exe", "gcc"),
    ];
    #[cfg(not(windows))]
    let c_candidates = [
        ("cc", "cc"),
        ("clang", "clang"),
        ("gcc", "gcc"),
    ];

    let mut toolchain = None;
    let mut cpp_compiler = None;
    for (bin, kind) in cpp_candidates {
        if let Some(path) = find_in_path(bin) {
            cpp_compiler = Some(path);
            toolchain = Some(kind.to_string());
            break;
        }
    }
    let mut c_compiler = None;
    for (bin, kind) in c_candidates {
        if let Some(path) = find_in_path(bin) {
            c_compiler = Some(path);
            if toolchain.is_none() {
                toolchain = Some(kind.to_string());
            }
            break;
        }
    }

    ToolchainDetection {
        c_compiler,
        cpp_compiler,
        toolchain,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Only init default logging if NOT bridge/dev
    if !matches!(cli.command, Some(Commands::Dev)) {
         tracing_subscriber::fmt::init();
    }

    // Determine Root (Parent of toolchain or CWD)
    // Assuming toolchain is running from Lunu/toolchain, root is Lunu/.. (Libs folder)
    // But user input implies running in Lunu folder. Let's find .luaurc or use CWD.
    let cwd = std::env::current_dir()?;
    // Search up for .luaurc
    let root = find_root(&cwd).unwrap_or(cwd.clone());
    
    // Don't print "Lunu Root" for bridge/dev command to keep stdout clean
    if !matches!(cli.command, Some(Commands::Dev)) && cli.command.is_some() {
        println!("Lunu Root: {:?}", root);
    }

    match cli.command {
        None => {
            // Check if we are installed
            if is_installed()? {
                 println!("Lunu is installed! Run 'lunu --help' to see available commands.");
            } else {
                 install_self().await?;
            }
        },
        Some(Commands::Init) => {
            init_project(&cwd).await?;
        },
        Some(Commands::Create { name }) => {
            create_project(&cwd, &name).await?;
        },
        Some(Commands::Install) => {
            install_from_config(&root).await?;
        },
        Some(Commands::Remove { lib }) => {
            remove_dependency(&root, &lib).await?;
        },
        Some(Commands::Update { lib }) => {
            update_dependencies(&root, lib.as_deref()).await?;
        },
        Some(Commands::List) => {
            list_dependencies(&root).await?;
        },
        Some(Commands::Package) => {
            package_project(&root).await?;
        },
        Some(Commands::Check) => {
            check_environment(&root).await?;
        },
        Some(Commands::Dev) => {
            println!("Starting Lunu Dev Server...");
            bridge_server::run().await?;
        },
        Some(Commands::Build { script, output, force, open, icon, open_cmd }) => {
            check_bridge_dependencies(&root).await;
            let runtime = resolve_runtime_for_root(&root).await?;
            match runtime {
                RuntimeKind::Lute => {
                    build_with_lute(&root, &script, output, open, &icon, &open_cmd)?;
                }
                RuntimeKind::Lune => {
                    let runtime_path = runtime_cache_bin(RuntimeTarget::Lune);
                    let final_path = if runtime_path.exists() {
                        Some(runtime_path)
                    } else {
                        find_lune_executable(&root)
                    };
                    lunu_builder::build_executable(&script, output, force, open, icon, open_cmd, final_path)?;
                }
            }
        },
        Some(Commands::Scaffold { name, template }) => {
            scaffold_project(&cwd, &name, template).await?;
        },
        Some(Commands::Module { name, lang }) => {
            create_module(&root, &name, lang).await?;
        },
        Some(Commands::Profile { script, runs }) => {
            profile_script(&root, &script, runs)?;
        },
        Some(Commands::Run { script, args }) => {
            let runtime = resolve_runtime_for_root(&root).await?;
            ensure_runtime_aliases(&root, runtime).await?;
            maybe_prompt_update(runtime_target_from_kind(runtime)).await?;
            run_script(&root, &script, &args, runtime)?;
        },
        Some(Commands::Test { file }) => {
            let runtime = resolve_runtime_for_root(&root).await?;
            run_tests(&root, file, runtime).await?;
        },
        Some(Commands::Runtime { runtime, update }) => {
            if update {
                update_runtime(runtime).await?;
            } else {
                let path = runtime_cache_bin(runtime);
                let meta = read_runtime_meta(runtime);
                if let Some(meta) = meta {
                    println!("{} runtime: {} ({}) at {:?}", runtime_name(runtime), meta.version, meta.source, path);
                } else if path.exists() {
                    println!("{} runtime: installed at {:?}", runtime_name(runtime), path);
                } else {
                    println!("{} runtime: not installed. Use --update to fetch.", runtime_name(runtime));
                }
            }
        },
        Some(Commands::Runtimes { update }) => {
            if update {
                update_runtime(RuntimeTarget::Lute).await?;
                update_runtime(RuntimeTarget::Lune).await?;
            } else {
                let lute_path = runtime_cache_bin(RuntimeTarget::Lute);
                let lune_path = runtime_cache_bin(RuntimeTarget::Lune);
                let lute_meta = read_runtime_meta(RuntimeTarget::Lute);
                let lune_meta = read_runtime_meta(RuntimeTarget::Lune);
                if let Some(meta) = lute_meta {
                    println!("lute runtime: {} ({}) at {:?}", meta.version, meta.source, lute_path);
                } else if lute_path.exists() {
                    println!("lute runtime: installed at {:?}", lute_path);
                } else {
                    println!("lute runtime: not installed. Use --update to fetch.");
                }
                if let Some(meta) = lune_meta {
                    println!("lune runtime: {} ({}) at {:?}", meta.version, meta.source, lune_path);
                } else if lune_path.exists() {
                    println!("lune runtime: installed at {:?}", lune_path);
                } else {
                    println!("lune runtime: not installed. Use --update to fetch.");
                }
            }
        },
        Some(Commands::Add { query, alias }) => {
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
            let runtime = resolve_runtime_for_root(&root).await?;
            let build_cfg = Some(build_config_for(runtime, None));
            let mut proj = load_or_init_project(&root, &config_path, runtime, build_cfg).await?;
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
        },
        Some(Commands::Clean) => {
            let cache_dir = runtime_cache_dir();
            if cache_dir.exists() {
                println!("Cleaning cache at {:?}...", cache_dir);
                async_fs::remove_dir_all(&cache_dir).await?;
                println!("Cache cleaned.");
            } else {
                println!("Cache is already empty.");
            }
        },
        Some(Commands::Upgrade) => {
            self_update().await?;
        },
        Some(Commands::Uninstall) => {
            self_uninstall().await?;
        }
    }

    Ok(())
}

async fn self_update() -> Result<()> {
    println!("Checking for updates...");
    let client = reqwest::Client::new();
    let resp = client.get("https://api.github.com/repos/tlipe/Lunu/releases/latest")
        .header("User-Agent", "Lunu-CLI")
        .send()
        .await?
        .json::<Value>()
        .await?;

    let latest_tag = resp["tag_name"].as_str().ok_or(anyhow::anyhow!("Failed to parse release tag"))?;
    let current_version = env!("CARGO_PKG_VERSION");
    let current_tag = format!("v{}", current_version);

    if latest_tag == current_tag {
        println!("Lunu is already up to date ({})", current_tag);
        return Ok(());
    }

    println!("New version available: {} (Current: {})", latest_tag, current_tag);
    println!("Updating...");

    // Find asset
    let assets = resp["assets"].as_array().ok_or(anyhow::anyhow!("No assets found"))?;
    let mut candidates: Vec<(String, String)> = assets
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            let url = a["browser_download_url"].as_str()?.to_string();
            let name_lower = name.to_lowercase();
            if !name_lower.contains("lunu") || !asset_extension_supported(&name_lower) {
                return None;
            }
            Some((name, url))
        })
        .collect();
    if candidates.is_empty() {
        return Err(anyhow::anyhow!("No compatible assets found"));
    }
    candidates.sort_by_key(|(name, _)| name.to_lowercase());
    let expected = lunu_bin_filename();
    let mut picked = candidates
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(&expected))
        .cloned();
    if picked.is_none() {
        for require_arch in [true, false] {
            if let Some(item) = candidates.iter().find(|(name, _)| {
                let n = name.to_lowercase();
                asset_matches_platform(&n, require_arch)
            }) {
                picked = Some(item.clone());
                break;
            }
        }
    }
    let (asset_name, download_url) = picked.unwrap_or_else(|| candidates[0].clone());

    let bytes = client.get(&download_url).send().await?.bytes().await?;
    
    let current_exe = std::env::current_exe()?;
    let old_exe = old_exe_path(&current_exe);
    
    if old_exe.exists() {
        let _ = async_fs::remove_file(&old_exe).await;
    }
    async_fs::rename(&current_exe, &old_exe).await?;
    
    let mut content = bytes.to_vec();
    let asset_lower = asset_name.to_lowercase();
    let bin_candidates = vec![
        lunu_bin_filename(),
        "lunu".to_string(),
        "lunu.exe".to_string(),
    ];
    if asset_lower.ends_with(".zip") {
        content = extract_binary_from_zip(&content, &bin_candidates)?;
    } else if asset_lower.ends_with(".tar.gz") || asset_lower.ends_with(".tgz") {
        content = extract_binary_from_tar_gz(&content, &bin_candidates)?;
    }
    async_fs::write(&current_exe, content).await?;
    ensure_executable(&current_exe)?;
    
    println!("Updated successfully to {}!", latest_tag);
    Ok(())
}

async fn self_uninstall() -> Result<()> {
    println!("Uninstalling Lunu...");
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let install_dir = home_dir.join(".lunu");

    // Remove from PATH
    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(env) = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE) {
            if let Ok(path_val) = env.get_value::<String, _>("Path") {
                let bin_dir = install_dir.join("bin");
                let bin_dir_str = bin_dir.to_str().unwrap_or("");
                
                if !bin_dir_str.is_empty() && path_val.to_lowercase().contains(&bin_dir_str.to_lowercase()) {
                    let new_path = path_val.split(';')
                        .filter(|p| !p.to_lowercase().contains(&bin_dir_str.to_lowercase()))
                        .collect::<Vec<_>>()
                        .join(";");
                    let _ = env.set_value("Path", &new_path);
                    println!("Removed from PATH.");
                }
            }
        }
    }

    // Remove directory
    if install_dir.exists() {
        // Self-deletion check
        let current_exe = std::env::current_exe()?;
        if current_exe.starts_with(&install_dir) {
             // Copy self to temp to finish uninstallation
             // Actually, we can just schedule deletion on reboot or use a batch script
             // But simpler: just rename self and try to delete dir, ignoring self error
            let old_exe = old_exe_path(&current_exe);
            let _ = async_fs::rename(&current_exe, &old_exe).await;
            #[cfg(windows)]
            {
                let cmd_script = format!("timeout /t 2 /nobreak > NUL & rmdir /s /q \"{}\"", install_dir.display());
                Command::new("cmd")
                   .arg("/C")
                   .arg(cmd_script)
                   .spawn()?;
                println!("Uninstall scheduled. Please exit the terminal.");
                std::process::exit(0);
            }
            #[cfg(not(windows))]
            {
                async_fs::remove_dir_all(&install_dir).await?;
            }
        } else {
             async_fs::remove_dir_all(&install_dir).await?;
        }
        println!("Removed .lunu directory.");
    }

    println!("Lunu uninstalled successfully.");
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
    if root.join("config").join("settings.json").exists() {
        return root.to_path_buf();
    }
    let lunu_sub = root.join("Lunu");
    if lunu_sub.exists() && lunu_sub.is_dir() {
        return lunu_sub;
    }
    root.to_path_buf()
}

fn project_name_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("lunu-project")
        .to_string()
}

// --- Installer Logic ---

fn is_installed() -> Result<bool> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let install_dir = home_dir.join(".lunu").join("bin");
    let current_exe = std::env::current_exe()?;
    
    // Check if we are running from the install directory
    Ok(current_exe.starts_with(&install_dir))
}

async fn install_self() -> Result<()> {
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let install_dir = home_dir.join(".lunu").join("bin");

    println!("Lunu Installer v0.2.0 (Single Binary)");
    println!("=====================================");
    println!("Target: {:?}", install_dir);

    // 1. Create Install Directory
    if !install_dir.exists() {
        println!("Creating install directory...");
        async_fs::create_dir_all(&install_dir).await?;
    }

    // 2. Extract Binaries
    println!("Extracting binaries...");
    
    // Copy Self
    let current_exe = std::env::current_exe()?;
    let target_lunu = install_dir.join(lunu_bin_filename());
    
    println!("  Copying {}...", lunu_bin_filename());
    async_fs::copy(&current_exe, &target_lunu).await?;
    ensure_executable(&target_lunu)?;

    // 3. Setup PATH
    println!("Setting up PATH...");
    setup_path(&install_dir)?;

    println!("\nInstallation Successful! 🎉");
    println!("Please restart your terminal (or VS Code) for changes to take effect.");
    println!("Try running: lunu --help");

    // Pause before exit
    print!("\nPress Enter to exit...");
    std::io::stdout().flush()?;
    let _ = std::io::stdin().read_line(&mut String::new());

    Ok(())
}

#[cfg(windows)]
fn setup_path(bin_dir: &Path) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu.open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    
    let path_val: String = env.get_value("Path")?;
    let bin_dir_str = bin_dir.to_str().unwrap();

    // Normalize paths for comparison (remove trailing slashes, lowercase)
    let normalized_path_val = path_val.to_lowercase();
    let normalized_bin_dir = bin_dir_str.to_lowercase();

    if !normalized_path_val.contains(&normalized_bin_dir) {
        let new_path = if path_val.ends_with(';') {
            format!("{}{}", path_val, bin_dir_str)
        } else {
            format!("{};{}", path_val, bin_dir_str)
        };
        env.set_value("Path", &new_path)?;
        println!("Added to PATH.");
    } else {
        println!("Already in PATH.");
    }
    
    // Broadcast setting change
    use winapi::um::winuser::{SendMessageTimeoutA, HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG};
    use std::ptr;
    use std::ffi::CString;

    unsafe {
        let env_str = CString::new("Environment").unwrap();
        SendMessageTimeoutA(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            env_str.as_ptr() as _,
            SMTO_ABORTIFHUNG,
            5000,
            ptr::null_mut(),
        );
    }

    Ok(())
}

#[cfg(not(windows))]
fn setup_path(_bin_dir: &Path) -> Result<()> {
    println!("Manual PATH configuration required for non-Windows systems.");
    Ok(())
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

async fn load_or_init_project(
    root: &Path,
    config_path: &Path,
    runtime: RuntimeKind,
    build: Option<BuildConfig>,
) -> Result<ProjectConfig> {
    let mut cfg = if config_path.exists() {
        ProjectConfig::load(config_path).await?
    } else {
        let name = project_name_from_root(root);
        let cfg = ProjectConfig::new_with_runtime(name.as_str(), runtime_config_for(runtime), build.clone());
        cfg.save(config_path).await?;
        cfg
    };

    if cfg.runtime.is_none() {
        cfg.runtime = Some(runtime_config_for(runtime));
    }
    if cfg.build.is_none() {
        cfg.build = build;
    }

    Ok(cfg)
}

fn main_template(runtime: RuntimeKind) -> String {
    match runtime {
        RuntimeKind::Lute => [
            "local process = require(\"@lute/process\")",
            "local path = require(\"@std/path\")",
            "",
            "print(\"Hello from Lute\")",
            "print(`cwd: {process.cwd()}`)",
            "print(`exe: {process.execpath()}`)",
            "",
        ]
        .join("\n"),
        RuntimeKind::Lune => "print(\"Hello from Lunu\")\n".to_string(),
    }
}

async fn ensure_project_files(root: &Path, runtime: RuntimeKind) -> Result<()> {
    let src_dir = root.join("src");
    let modules_dir = root.join("modules");
    let config_dir = root.join("config");
    let native_dir = root.join("native");
    let build_dir = root.join("build");
    
    if !src_dir.exists() {
        async_fs::create_dir_all(&src_dir).await?;
    }
    if !modules_dir.exists() {
        async_fs::create_dir_all(&modules_dir).await?;
    }
    if !config_dir.exists() {
        async_fs::create_dir_all(&config_dir).await?;
    }
    if runtime == RuntimeKind::Lute {
        if !native_dir.exists() {
            async_fs::create_dir_all(&native_dir).await?;
        }
        if !build_dir.exists() {
            async_fs::create_dir_all(&build_dir).await?;
        }
    }

    let main_path = src_dir.join("main.luau");
    if !main_path.exists() {
        async_fs::write(&main_path, main_template(runtime)).await?;
    }

    let settings_path = config_dir.join("settings.json");
    if !settings_path.exists() {
        let default_settings = serde_json::json!({
            "server": {
                "host": "127.0.0.1",
                "http_port": 8000,
                "ssl_enabled": false,
                "ssl_cert_path": "",
                "ssl_key_path": ""
            },
            "security": {
                "auth_enabled": true,
                "allowed_hosts": ["127.0.0.1", "localhost"]
            },
            "logging": {
                "level": "info",
                "file": "logs/server.log"
            }
        });
        async_fs::write(&settings_path, serde_json::to_string_pretty(&default_settings)?).await?;
    }

    Ok(())
}

async fn update_luaurc(root: &Path, deps: &BTreeMap<String, DependencySpec>, runtime: RuntimeKind) -> Result<()> {
    let config_path = root.join(".luaurc");
    let mut luaurc = Luaurc::load(&config_path).await?;
    let path_to_alias = |path: &Path| -> String {
        path.to_string_lossy().replace("\\", "/") + "/"
    };
    if runtime == RuntimeKind::Lute {
        luaurc.remove_alias("@lute");
        luaurc.remove_alias("@std");
        let mut added = false;
        let runtime_root = runtime_lib_root(RuntimeTarget::Lute);
        let lute_dir = runtime_root.join("lute");
        let std_dir = runtime_root.join("std");
        let lute_std_libs = runtime_root.join("lute").join("std").join("libs");
        if lute_std_libs.exists() {
            luaurc.add_alias("lute", &path_to_alias(&lute_std_libs));
            added = true;
        } else if lute_dir.exists() {
            luaurc.add_alias("lute", &path_to_alias(&lute_dir));
            added = true;
        }
        if std_dir.exists() {
            luaurc.add_alias("std", &path_to_alias(&std_dir));
            added = true;
        } else if lute_std_libs.exists() {
            luaurc.add_alias("std", &path_to_alias(&lute_std_libs));
            added = true;
        }
        if !added {
            let source_root = ensure_lute_sources().await?;
            let lute_src = source_root.join("lute");
            let std_src = source_root.join("std");
            let source_lute_std_libs = source_root.join("lute").join("std").join("libs");
            if source_lute_std_libs.exists() {
                luaurc.add_alias("lute", &path_to_alias(&source_lute_std_libs));
                luaurc.add_alias("std", &path_to_alias(&source_lute_std_libs));
            } else {
                if lute_src.exists() {
                    luaurc.add_alias("lute", &path_to_alias(&lute_src));
                }
                if std_src.exists() {
                    luaurc.add_alias("std", &path_to_alias(&std_src));
                }
            }
        }
    }
    if runtime == RuntimeKind::Lune {
        let lunu_alias = if root.join("Lunu").exists() {
            "Lunu/".to_string()
        } else if root.parent().map(|p| p.join("Lunu").exists()).unwrap_or(false) {
            "../Lunu/".to_string()
        } else {
            "Lunu/".to_string()
        };
        luaurc.add_alias("lunu", &lunu_alias);
    }
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

async fn ensure_runtime_aliases(root: &Path, runtime: RuntimeKind) -> Result<()> {
    let config_path = project_config_path(root);
    if !config_path.exists() {
        return Ok(());
    }
    let cfg = ProjectConfig::load(&config_path).await?;
    update_luaurc(root, &cfg.dependencies, runtime).await?;
    Ok(())
}

async fn init_project(root: &Path) -> Result<()> {
    let runtime = select_runtime()?;
    ensure_runtime_available(root, runtime_target_from_kind(runtime)).await?;
    let toolchain = if runtime == RuntimeKind::Lute {
        if find_lute_executable(root).is_none() {
            return Err(anyhow::anyhow!(format!(
                "Lute runtime not found. Set LUTE_PATH, place bin/{} in the project, or add {} to PATH.",
                runtime_bin_filename(RuntimeTarget::Lute),
                runtime_bin_filename(RuntimeTarget::Lute)
            )));
        }
        let detection = detect_cpp_toolchain();
        if detection.c_compiler.is_none() || detection.cpp_compiler.is_none() {
            return Err(anyhow::anyhow!("C/C++ compiler not found. Install MSVC Build Tools, clang, or gcc and ensure cl.exe/clang++.exe/g++.exe is on PATH."));
        }
        Some(detection)
    } else {
        None
    };

    let build_cfg = Some(build_config_for(runtime, toolchain));
    ensure_project_files(root, runtime).await?;
    
    if runtime == RuntimeKind::Lune {
        let modules_dir = root.join("modules");
        let lunu_mod_dir = modules_dir.join("lunu");
        if !lunu_mod_dir.exists() {
            async_fs::create_dir_all(&lunu_mod_dir).await?;
            let init_content = include_str!("../../init.luau");
            async_fs::write(lunu_mod_dir.join("init.luau"), init_content).await?;
            println!("Installed Lunu core library to modules/lunu");
        }
    }

    let config_path = project_config_path(root);
    let mut cfg = load_or_init_project(root, &config_path, runtime, build_cfg).await?;

    let discovered = scan_modules(root);
    for (name, spec) in discovered {
        cfg.add_dependency(&name, spec);
    }
    
    if runtime == RuntimeKind::Lune {
        let mut lunu_spec = DependencySpec::default();
        lunu_spec.path = Some("modules/lunu".to_string());
        cfg.add_dependency("lunu", lunu_spec);
    }

    cfg.save(&config_path).await?;

    update_luaurc(root, &cfg.dependencies, runtime).await?;
    
    if runtime == RuntimeKind::Lune {
        let luaurc_path = root.join(".luaurc");
        let mut luaurc = Luaurc::load(&luaurc_path).await?;
        luaurc.add_alias("lunu", "modules/lunu/");
        luaurc.save(&luaurc_path).await?;
    }

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

async fn scaffold_project(cwd: &Path, name: &str, template: TemplateKind) -> Result<()> {
    create_project(cwd, name).await?;
    let project_dir = cwd.join(name);
    let config_path = project_config_path(&project_dir);
    if config_path.exists() {
        if let Ok(cfg) = ProjectConfig::load(&config_path).await {
            if cfg.runtime.as_ref().map(|r| r.name.as_str()) == Some("lute") {
                println!("Scaffold created at {:?}", project_dir);
                return Ok(());
            }
        }
    }
    let main_path = project_dir.join("src").join("main.luau");
    let content = match template {
        TemplateKind::App => "print(\"Hello from Lunu\")\n".to_string(),
        TemplateKind::Game => {
            [
                "local frame = 0",
                "while frame < 3 do",
                "    print(\"tick\", frame)",
                "    frame = frame + 1",
                "end",
                "",
            ]
            .join("\n")
        }
    };
    async_fs::write(&main_path, content).await?;
    println!("Scaffold created at {:?}", project_dir);
    Ok(())
}

async fn create_module(root: &Path, name: &str, lang: ModuleLang) -> Result<()> {
    let modules_dir = root.join("modules");
    async_fs::create_dir_all(&modules_dir).await?;
    let module_dir = modules_dir.join(name);
    if module_dir.exists() {
        return Err(anyhow::anyhow!("Module '{}' already exists", name));
    }
    async_fs::create_dir_all(&module_dir).await?;

    let (worker_name, worker_content) = match lang {
        ModuleLang::Python => (
            "worker.py",
            [
                "import sys",
                "import json",
                "",
                "def normalize_params(params):",
                "    if len(params) == 1 and isinstance(params[0], list):",
                "        return params[0]",
                "    return params",
                "",
                "def handle(method, params):",
                "    params = normalize_params(params)",
                "    if method == \"greet\":",
                "        name = params[0] if len(params) > 0 else \"\"",
                "        return {\"result\": f\"Hello from Python, {name}!\"}",
                "    if method == \"echo\":",
                "        return {\"result\": params[0] if len(params) > 0 else None}",
                "    return {\"error\": {\"code\": \"method_not_found\", \"message\": \"Method not found\"}}",
                "",
                "def main():",
                "    while True:",
                "        line = sys.stdin.readline()",
                "        if line == \"\":",
                "            break",
                "        line = line.strip()",
                "        if line == \"\":",
                "            continue",
                "        try:",
                "            payload = json.loads(line)",
                "        except Exception:",
                "            continue",
                "        request_id = payload.get(\"id\")",
                "        method = payload.get(\"method\")",
                "        params = payload.get(\"params\", [])",
                "        if request_id is None or method is None:",
                "            continue",
                "        response = handle(method, params)",
                "        response[\"id\"] = request_id",
                "        sys.stdout.write(json.dumps(response) + \"\\n\")",
                "        sys.stdout.flush()",
                "",
                "if __name__ == \"__main__\":",
                "    main()",
                "",
            ]
            .join("\n"),
        ),
        ModuleLang::Node => (
            "worker.js",
            [
                "const readline = require('readline');",
                "const rl = readline.createInterface({input: process.stdin, output: process.stdout});",
                "",
                "const normalizeParams = (params) => {",
                "    if (Array.isArray(params) && params.length === 1 && Array.isArray(params[0])) {",
                "        return params[0];",
                "    }",
                "    return params;",
                "};",
                "",
                "const handle = (method, params) => {",
                "    const normalized = normalizeParams(params || []);",
                "    if (method === 'greet') {",
                "        const name = normalized.length > 0 ? normalized[0] : '';",
                "        return {result: `Hello from Node.js, ${name}!`};",
                "    }",
                "    if (method === 'echo') {",
                "        return {result: normalized.length > 0 ? normalized[0] : null};",
                "    }",
                "    return {error: {code: 'method_not_found', message: 'Method not found'}};",
                "};",
                "",
                "rl.on('line', (line) => {",
                "    if (!line) return;",
                "    const msg = JSON.parse(line);",
                "    const response = {id: msg.id};",
                "    if (!msg.method) {",
                "        response.error = {code: 'invalid_request', message: 'Missing method'};",
                "        console.log(JSON.stringify(response));",
                "        return;",
                "    }",
                "    const result = handle(msg.method, msg.params || []);",
                "    if (result.error) {",
                "        response.error = result.error;",
                "    } else {",
                "        response.result = result.result;",
                "    }",
                "    console.log(JSON.stringify(response));",
                "});",
                "",
            ]
            .join("\n"),
        ),
        ModuleLang::Rust => (
            "src/main.rs",
            [
                "use std::io::{self, BufRead, Write};",
                "use serde::{Deserialize, Serialize};",
                "use serde_json::Value;",
                "",
                "#[derive(Deserialize)]",
                "struct Request {",
                "    id: Option<String>,",
                "    method: String,",
                "    params: Option<Value>,",
                "}",
                "",
                "#[derive(Serialize)]",
                "struct Response {",
                "    id: Option<String>,",
                "    #[serde(skip_serializing_if = \"Option::is_none\")]",
                "    result: Option<Value>,",
                "    #[serde(skip_serializing_if = \"Option::is_none\")]",
                "    error: Option<ErrorVal>,",
                "}",
                "",
                "#[derive(Serialize)]",
                "struct ErrorVal {",
                "    code: String,",
                "    message: String,",
                "}",
                "",
                "fn main() {",
                "    let stdin = io::stdin();",
                "    let mut stdout = io::stdout();",
                "    for line in stdin.lock().lines() {",
                "        if let Ok(line) = line {",
                "            if line.trim().is_empty() { continue; }",
                "            if let Ok(req) = serde_json::from_str::<Request>(&line) {",
                "                let res = handle(req);",
                "                if let Ok(json) = serde_json::to_string(&res) {",
                "                    writeln!(stdout, \"{}\", json).ok();",
                "                    stdout.flush().ok();",
                "                }",
                "            }",
                "        }",
                "    }",
                "}",
                "",
                "fn handle(req: Request) -> Response {",
                "    match req.method.as_str() {",
                "        \"greet\" => {",
                "            let name = req.params.as_ref()",
                "                .and_then(|v| v.as_array())",
                "                .and_then(|a| a.get(0))",
                "                .and_then(|v| v.as_str())",
                "                .unwrap_or(\"\");",
                "            Response {",
                "                id: req.id,",
                "                result: Some(Value::String(format!(\"Hello from Rust, {}!\", name))),",
                "                error: None,",
                "            }",
                "        },",
                "        \"echo\" => {",
                "            let arg = req.params.as_ref()",
                "                .and_then(|v| v.as_array())",
                "                .and_then(|a| a.get(0))",
                "                .cloned();",
                "            Response {",
                "                id: req.id,",
                "                result: arg,",
                "                error: None,",
                "            }",
                "        },",
                "        _ => Response {",
                "            id: req.id,",
                "            result: None,",
                "            error: Some(ErrorVal { code: \"404\".into(), message: \"Method not found\".into() }),",
                "        }",
                "    }",
                "}",
            ].join("\n"),
        ),
    };

    if matches!(lang, ModuleLang::Rust) {
        let src_dir = module_dir.join("src");
        async_fs::create_dir_all(&src_dir).await?;
        
        let cargo_toml = [
            "[package]",
            &format!("name = \"{}\"", name),
            "version = \"0.1.0\"",
            "edition = \"2021\"",
            "",
            "[dependencies]",
            "serde = { version = \"1.0\", features = [\"derive\"] }",
            "serde_json = \"1.0\"",
        ].join("\n");
        
        async_fs::write(module_dir.join("Cargo.toml"), cargo_toml).await?;
        async_fs::write(src_dir.join("main.rs"), worker_content).await?;
    } else {
        let worker_path = module_dir.join(worker_name);
        async_fs::write(&worker_path, worker_content).await?;
    }

    let rust_worker_cmd = if let Some(ext) = executable_extension() {
        format!("target/release/{}.{}", name, ext)
    } else {
        format!("target/release/{}", name)
    };
    let bridge_json = match lang {
        ModuleLang::Python => serde_json::json!({
            "protocol": "lunu-worker-v1",
            "worker": { "cmd": ["python", worker_name], "cwd": ".", "env": {} },
            "methods": { "greet": {}, "echo": {} }
        }),
        ModuleLang::Node => serde_json::json!({
            "protocol": "lunu-worker-v1",
            "worker": { "cmd": ["node", worker_name], "cwd": ".", "env": {} },
            "methods": { "greet": {}, "echo": {} }
        }),
        ModuleLang::Rust => serde_json::json!({
            "protocol": "lunu-worker-v1",
            "worker": { 
                "cmd": [rust_worker_cmd], 
                "cwd": ".", 
                "env": {},
                "notes": "Run 'cargo build --release' in this folder to build the worker."
            },
            "methods": { "greet": {}, "echo": {} }
        }),
    };
    let bridge_path = module_dir.join("bridge.json");
    async_fs::write(&bridge_path, serde_json::to_string_pretty(&bridge_json)?).await?;
    println!("Module created at {:?}", module_dir);
    Ok(())
}

fn profile_script(root: &Path, script: &Path, runs: u32) -> Result<()> {
    let lune = find_lune_executable(root).ok_or_else(|| anyhow::anyhow!("Lune runtime not found. Set LUNE_PATH or add to PATH."))?;
    let mut durations = Vec::new();
    for _ in 0..runs.max(1) {
        let start = std::time::Instant::now();
        let status = Command::new(&lune)
            .arg("run")
            .arg(script)
            .current_dir(root)
            .status()
            .with_context(|| "Failed to run lune")?;
        if !status.success() {
            return Err(anyhow::anyhow!("Lune run failed"));
        }
        durations.push(start.elapsed());
    }
    let total_ms: u128 = durations.iter().map(|d| d.as_millis()).sum();
    let avg_ms = total_ms as f64 / durations.len() as f64;
    println!("Runs: {}", durations.len());
    println!("Total: {} ms", total_ms);
    println!("Average: {:.2} ms", avg_ms);
    Ok(())
}

fn find_lune_executable(root: &Path) -> Option<PathBuf> {
    let local = root.join("bin").join(runtime_bin_filename(RuntimeTarget::Lune));
    if local.exists() {
        return Some(local);
    }
    if let Ok(path) = std::env::var("LUNE_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let cached = runtime_cache_bin(RuntimeTarget::Lune);
    if cached.exists() {
        return Some(cached);
    }
    find_in_path(&runtime_bin_filename(RuntimeTarget::Lune))
}

fn find_lute_executable(root: &Path) -> Option<PathBuf> {
    let local = root.join("bin").join(runtime_bin_filename(RuntimeTarget::Lute));
    if local.exists() {
        return Some(local);
    }
    if let Ok(path) = std::env::var("LUTE_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    let cached = runtime_cache_bin(RuntimeTarget::Lute);
    if cached.exists() {
        return Some(cached);
    }
    if let Some(p) = find_in_path(&runtime_bin_filename(RuntimeTarget::Lute)) {
        return Some(p);
    }
    ensure_embedded_lute()
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for path in std::env::split_paths(&paths) {
        let candidate = path.join(binary);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn embedded_lute_bytes() -> Option<&'static [u8]> {
    #[cfg(windows)]
    {
        Some(include_bytes!("../resources/lute.exe"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

async fn run_tests(root: &Path, specific_file: Option<PathBuf>, runtime: RuntimeKind) -> Result<()> {
    println!("Running tests using {} runtime...", match runtime { RuntimeKind::Lute => "Lute", RuntimeKind::Lune => "Lune" });
    
    let mut test_files = Vec::new();
    
    if let Some(f) = specific_file {
        if !f.exists() {
             return Err(anyhow::anyhow!("Test file not found: {:?}", f));
        }
        test_files.push(f);
    } else {
        // Scan for *.test.luau and *.spec.luau
        let mut dirs = vec![root.to_path_buf()];
        while let Some(dir) = dirs.pop() {
            let mut entries = match async_fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(_) => continue,
            };
            
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    if name != "node_modules" && name != "target" && name != ".git" && name != "dist" {
                        dirs.push(path);
                    }
                } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.ends_with(".test.luau") || name.ends_with(".spec.luau") {
                        test_files.push(path);
                    }
                }
            }
        }
    }

    if test_files.is_empty() {
        println!("No test files found.");
        return Ok(());
    }

    println!("Found {} test file(s).", test_files.len());
    let mut failed = 0;

    for file in test_files {
        print!("Testing {:?} ... ", file.file_name().unwrap());
        io::stdout().flush()?;
        
        let start = std::time::Instant::now();
        let status = match runtime {
            RuntimeKind::Lute => {
                let lute = find_lute_executable(root).ok_or_else(|| anyhow::anyhow!("Lute not found"))?;
                Command::new(&lute)
                    .arg("run")
                    .arg(&file)
                    .current_dir(root)
                    .output()
                    .with_context(|| "Failed to run lute")?
            }
            RuntimeKind::Lune => {
                let lune = find_lune_executable(root).ok_or_else(|| anyhow::anyhow!("Lune not found"))?;
                Command::new(&lune)
                    .arg("run")
                    .arg(&file)
                    .current_dir(root)
                    .output()
                    .with_context(|| "Failed to run lune")?
            }
        };

        let duration = start.elapsed();
        if status.status.success() {
            println!("OK ({:.2?})", duration);
        } else {
            println!("FAIL ({:.2?})", duration);
            println!("--- Output ---");
            io::stdout().write_all(&status.stdout)?;
            io::stderr().write_all(&status.stderr)?;
            println!("--------------");
            failed += 1;
        }
    }

    if failed > 0 {
        return Err(anyhow::anyhow!("{} test(s) failed.", failed));
    }
    
    println!("All tests passed!");
    Ok(())
}

fn run_script(root: &Path, script: &Path, args: &[String], runtime: RuntimeKind) -> Result<()> {
    if !script.exists() {
        return Err(anyhow::anyhow!(format!("Input script not found: {:?}", script)));
    }
    let status = match runtime {
        RuntimeKind::Lute => {
            let lute = find_lute_executable(root).ok_or_else(|| anyhow::anyhow!(format!(
                "Lute runtime not found. Set LUTE_PATH, place bin/{} in the project, or add {} to PATH.",
                runtime_bin_filename(RuntimeTarget::Lute),
                runtime_bin_filename(RuntimeTarget::Lute)
            )))?;
            Command::new(&lute)
                .arg("run")
                .arg(script)
                .args(args)
                .current_dir(root)
                .status()
                .with_context(|| "Failed to run lute")?
        }
        RuntimeKind::Lune => {
            let lune = find_lune_executable(root).ok_or_else(|| anyhow::anyhow!("Lune runtime not found. Set LUNE_PATH or add to PATH."))?;
            Command::new(&lune)
                .arg("run")
                .arg(script)
                .args(args)
                .current_dir(root)
                .status()
                .with_context(|| "Failed to run lune")?
        }
    };
    if !status.success() {
        return Err(anyhow::anyhow!("Script execution failed"));
    }
    Ok(())
}

fn build_with_lute(
    root: &Path,
    script: &Path,
    output: Option<PathBuf>,
    open: bool,
    _icon: &Option<PathBuf>,
    _open_cmd: &Option<bool>,
) -> Result<()> {
    let lute = find_lute_executable(root).ok_or_else(|| anyhow::anyhow!(format!(
        "Lute runtime not found. Set LUTE_PATH, place bin/{} in the project, or add {} to PATH.",
        runtime_bin_filename(RuntimeTarget::Lute),
        runtime_bin_filename(RuntimeTarget::Lute)
    )))?;
    if !script.exists() {
        return Err(anyhow::anyhow!(format!("Input script not found: {:?}", script)));
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| root.to_path_buf());
    let default_out = {
        let stem = script.file_stem().unwrap_or_default();
        let mut p = cwd.join(stem);
        if let Some(ext) = executable_extension() {
            p.set_extension(ext);
        }
        p
    };
    let out_path = output.clone().unwrap_or(default_out);
    let status = Command::new(&lute)
        .arg("compile")
        .arg(script)
        .arg("--output")
        .arg(&out_path)
        .current_dir(root)
        .status()
        .with_context(|| "Failed to run lute compile")?;
    if !status.success() {
        return Err(anyhow::anyhow!("Lute compile failed"));
    }
    if open {
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("explorer")
                .arg("/select,")
                .arg(&out_path)
                .spawn();
        }
    }
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

    update_luaurc(root, &cfg.dependencies, runtime_kind_from_config(&cfg)).await?;
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
    let exe_path = if let Some(ext) = executable_extension() {
        root.join(format!("{}.{}", stem, ext))
    } else {
        root.join(stem)
    };
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

    if config_path.exists() {
        if let Ok(cfg) = ProjectConfig::load(&config_path).await {
            let runtime_kind = runtime_kind_from_config(&cfg);
            let _ = update_luaurc(root, &cfg.dependencies, runtime_kind).await;
            if let Some(runtime) = cfg.runtime {
                println!("- Runtime: {}", runtime.name);
                if runtime.name == "lute" {
                    let lute = find_lute_executable(root);
                    let toolchain = detect_cpp_toolchain();
                    let c_found = toolchain.c_compiler.is_some();
                    let cpp_found = toolchain.cpp_compiler.is_some();
                    println!("- Lute executable: {}", lute.is_some());
                    println!("- C compiler: {}", c_found);
                    println!("- C++ compiler: {}", cpp_found);
                    let entry = root.join(&cfg.project.entry);
                    if !entry.exists() {
                        return Err(anyhow::anyhow!("Entry file not found for Lute check: {:?}", entry));
                    }
                    if let Some(p) = &lute {
                        println!("  Path: {:?}", p);
                    }
                    let lute = lute.ok_or_else(|| anyhow::anyhow!("Lute runtime not found. Set LUTE_PATH, place bin/lute.exe in the project, or add lute.exe to PATH."))?;
                    let status = Command::new(&lute)
                        .arg("check")
                        .arg(&entry)
                        .current_dir(root)
                        .status()
                        .with_context(|| "Failed to run lute check")?;
                    if !status.success() {
                        return Err(anyhow::anyhow!("Lute check failed"));
                    }
                }
                if runtime.name == "lune" {
                    let lune_path = find_lune_executable(root);
                    println!("- Lune executable: {}", lune_path.is_some());
                    if let Some(p) = lune_path {
                        println!("  Path: {:?}", p);
                    }
                }
            }
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

async fn check_bridge_dependencies(root: &Path) {
    let modules_dir = root.join("modules");
    if !modules_dir.exists() {
        return;
    }

    let mut dir = match async_fs::read_dir(&modules_dir).await {
        Ok(d) => d,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            let bridge_json = path.join("bridge.json");
            if bridge_json.exists() {
                check_module_dependency(&path).await;
            }
        }
    }
}

async fn check_module_dependency(module_dir: &Path) {
    // Read bridge.json
    let content = match async_fs::read_to_string(module_dir.join("bridge.json")).await {
        Ok(c) => c,
        Err(_) => return,
    };
    
    let json: Value = match serde_json::from_str(&content) {
        Ok(j) => j,
        Err(_) => return,
    };
    
    if let Some(cmd_arr) = json.get("worker").and_then(|w| w.get("cmd")).and_then(|c| c.as_array()) {
        if let Some(cmd_val) = cmd_arr.get(0).and_then(|v| v.as_str()) {
            let cmd = cmd_val.to_string();
            // Check for common interpreters
            if ["python", "node", "npm", "cargo", "go", "ruby", "perl", "php", "java"].contains(&cmd.as_str()) {
                 let local_path = module_dir.join(&cmd);
                 let local_path_exe = module_dir.join(format!("{}.exe", cmd));
                 
                 if !local_path.exists() && !local_path_exe.exists() {
                     println!("WARN: Module '{}' depends on system command '{}'.", module_dir.file_name().unwrap().to_string_lossy(), cmd);
                     println!("      Ensure target users have '{}' installed, or bundle a portable version in the module folder.", cmd);
                 } else {
                     println!("INFO: Module '{}' uses bundled runtime for '{}'.", module_dir.file_name().unwrap().to_string_lossy(), cmd);
                 }
            }
        }
    }
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

    #[tokio::test]
    async fn init_project_creates_core_files() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        std::env::remove_var("LUNU_RUNTIME");
        std::env::remove_var("LUNU_INIT_RUNTIME");
        init_project(root).await.unwrap();

        assert!(root.join("lunu.toml").exists());
        assert!(root.join("lunu.lock").exists());
        assert!(root.join(".luaurc").exists());
        assert!(root.join("modules").join("lunu").join("init.luau").exists());
        assert!(root.join("src").join("main.luau").exists());
        assert!(root.join("config").join("settings.json").exists());
    }

    #[tokio::test]
    async fn package_project_creates_dist_bundle() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        std::env::remove_var("LUNU_RUNTIME");
        std::env::remove_var("LUNU_INIT_RUNTIME");
        init_project(root).await.unwrap();
        let exe_name = if let Some(ext) = executable_extension() {
            format!("main.{}", ext)
        } else {
            "main".to_string()
        };
        std::fs::write(root.join(&exe_name), "stub").unwrap();

        package_project(root).await.unwrap();

        let dist = root.join("dist");
        assert!(dist.exists());
        assert!(dist.join(&exe_name).exists());
        assert!(dist.join("lunu.toml").exists());
    }
}
