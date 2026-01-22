use clap::{Parser, Subcommand};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "lunu-build")]
#[command(about = "Lunu Builder - Create standalone executables")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build { script, output, force, open, icon, open_cmd } => {
            build_executable(&script, output, force, open, icon, open_cmd)?;
        }
    }
    Ok(())
}

fn build_executable(
    script: &Path,
    output: Option<PathBuf>,
    force: bool,
    open: bool,
    icon: Option<PathBuf>,
    open_cmd: Option<bool>,
) -> anyhow::Result<()> {
    println!("Lunu Builder v0.1.1 (Optimized)");
    println!("-------------------------------");

    if !script.exists() {
        return Err(anyhow::anyhow!("Input script not found: {:?}", script));
    }

    // 1. Determine Output Name
    // Rule: Output to Current Working Directory (CWD) where command is run
    let cwd = std::env::current_dir()?;
    let output_path = output.unwrap_or_else(|| {
        let stem = script.file_stem().unwrap_or_default();
        let mut p = cwd.join(stem); // Force CWD
        p.set_extension("exe");
        p
    });

    println!("[1/5] Target: {:?}", output_path);
    
    // 2. Locate Dependencies
    let self_exe = std::env::current_exe()?;
    let self_dir = self_exe.parent().unwrap();
    let stub_path = self_dir.join("lunu-stub.exe");
    
    if !stub_path.exists() {
        return Err(anyhow::anyhow!("Stub executable not found at {:?}. Please compile it first.", stub_path));
    }

    // 3. Cache System
    // We cache the "Runtime + Lunu Libs" part. The main script is always injected fresh.
    let cache_dir = dirs::cache_dir().unwrap_or(cwd.join(".cache")).join("lunu-builder");
    fs::create_dir_all(&cache_dir)?;
    
    // Invalidate cache if we changed the builder logic (e.g. by checking if cache is older than builder)
    // For now, just rely on FORCE flag.
    let cache_file = cache_dir.join("runtime_payload.zip");
    let use_cache = !force && cache_file.exists();

    let mut base_zip_buffer = Vec::new();

    if use_cache {
        println!("[2/5] Loading runtime from cache...");
        let mut f = File::open(&cache_file)?;
        f.read_to_end(&mut base_zip_buffer)?;
    } else {
        println!("[2/5] Building runtime payload (this takes a moment)...");
        
        let project_root = find_project_root(self_dir, &cwd)?;
        let lune_path = resolve_lune_path(&project_root)?;
        if !lune_path.exists() {
            return Err(anyhow::anyhow!("Lune runtime not found at {:?}.", lune_path));
        }

        // Build base ZIP (Lune + Lunu Libs)
        let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut base_zip_buffer));
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

        // Add Lune
        zip_writer.start_file("bin/lune.exe", options)?;
        let mut lune_content = Vec::new();
        File::open(&lune_path)?.read_to_end(&mut lune_content)?;
        zip_writer.write_all(&lune_content)?;

        // Add Lunu Bridge Files
        println!("[2/5] Lunu Root detected at: {:?}", project_root);

        let bridge_dir = project_root.join("src").join("bridge");
        let libs_dir = project_root.join("src").join("libs");
        let config_dir = project_root.join("config");
        let modules_dir = project_root.join("modules");

        if !modules_dir.exists() {
             return Err(anyhow::anyhow!("Modules directory not found at {:?}. Are you running the builder from the correct location?", modules_dir));
        }

        add_dir_to_zip(&mut zip_writer, &bridge_dir, "src/bridge", options)?;
        add_dir_to_zip(&mut zip_writer, &libs_dir, "src/libs", options)?;
        add_dir_to_zip(&mut zip_writer, &config_dir, "config", options)?;
        add_dir_to_zip(&mut zip_writer, &modules_dir, "modules", options)?;

        // Add rokit.toml
        let rokit_path = project_root.join("rokit.toml");
        if rokit_path.exists() {
            zip_writer.start_file("rokit.toml", options)?;
            let mut content = Vec::new();
            File::open(&rokit_path)?.read_to_end(&mut content)?;
            zip_writer.write_all(&content)?;
        }

        // Add .luaurc
        let luaurc_path = project_root.join(".luaurc");
        if luaurc_path.exists() {
            zip_writer.start_file(".luaurc", options)?;
            let mut content = Vec::new();
            File::open(&luaurc_path)?.read_to_end(&mut content)?;
            zip_writer.write_all(&content)?;
        }
        
        // Add init.luau (Root Lunu)
        let init_path = project_root.join("init.luau");
        if init_path.exists() {
             let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);
             
             // 1. Add as Lunu/init.luau (for require("@lunu"))
             zip_writer.start_file("Lunu/init.luau", options)?;
             let mut content = Vec::new();
             File::open(&init_path)?.read_to_end(&mut content)?;
             zip_writer.write_all(&content)?;

             // 2. Add as init.luau in root (fallback)
              zip_writer.start_file("init.luau", options)?;
              zip_writer.write_all(&content)?;
         } else {
             println!("Warning: init.luau not found at {:?}", init_path);
        }

        zip_writer.finish()?;
        drop(zip_writer); // Explicitly drop to release mutable borrow on base_zip_buffer
        
        // Save to cache
        let mut f = File::create(&cache_file)?;
        f.write_all(&base_zip_buffer)?;
    }

    println!("[3/5] Injecting user script...");
    
    // We need to append the user script to the ZIP. 
    // Since zip-rs doesn't support easy appending to existing buffer without re-reading,
    // and we want speed, we can cheat: 
    // Actually, appending to a ZIP requires parsing central dir.
    // Efficient approach:
    // 1. Load base ZIP.
    // 2. Open as ZipWriter in Append mode? zip-rs supports append to file, but memory buffer is tricky.
    // Simpler approach for reliability: Just decode base and re-encode? No, slow.
    // Correct approach for 'PyInstaller' style speed:
    // The stub reads the whole appended data. We can just append another ZIP? 
    // No, stub expects one valid ZIP.
    //
    // Let's rely on zip-rs 'Append' feature if possible, or just re-write the zip with pre-filled buffer.
    // zip-rs doesn't support append easily on buffers.
    //
    // Revised Strategy:
    // We kept the 'base_zip_buffer'. We open it with ZipWriter in Append mode?
    // Actually, simply re-creating the zip from files is fast enough if files are small.
    // But Lunu libs might be big.
    //
    // Let's stick to "Cache = File".
    // We copy cache file to a temp file, append script, then read back.
    
    let temp_zip_path = std::env::temp_dir().join(format!("lunu_build_{}.zip", uuid::Uuid::new_v4()));
    fs::copy(&cache_file, &temp_zip_path)?;
    
    let file = fs::OpenOptions::new().read(true).write(true).open(&temp_zip_path)?;
    let mut zip_writer = zip::ZipWriter::new_append(file)?;
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Stored);

    zip_writer.start_file("src/main.luau", options)?;
    let mut script_content = Vec::new();
    File::open(script)?.read_to_end(&mut script_content)?;
    zip_writer.write_all(&script_content)?;

    let open_cmd = open_cmd.unwrap_or(true);
    zip_writer.start_file("lunu_open_cmd.txt", options)?;
    if open_cmd {
        zip_writer.write_all(b"1")?;
    } else {
        zip_writer.write_all(b"0")?;
    }

    if let Some(icon_path) = icon {
        if !icon_path.exists() {
            return Err(anyhow::anyhow!("Icon file not found: {:?}", icon_path));
        }
        let icon_name = icon_path.file_name().and_then(|s| s.to_str()).unwrap_or("icon.ico");
        let icon_zip_path = format!("assets/{}", icon_name);
        zip_writer.start_file(icon_zip_path, options)?;
        let mut icon_content = Vec::new();
        File::open(&icon_path)?.read_to_end(&mut icon_content)?;
        zip_writer.write_all(&icon_content)?;
    }
    
    zip_writer.finish()?;

    println!("[4/5] Assembling executable...");

    // 4. Concatenate Stub + Final Zip
    let mut final_exe = File::create(&output_path)?;
    let mut stub_content = Vec::new();
    File::open(&stub_path)?.read_to_end(&mut stub_content)?;
    
    final_exe.write_all(&stub_content)?;
    
    let mut final_zip_content = Vec::new();
    File::open(&temp_zip_path)?.read_to_end(&mut final_zip_content)?;
    final_exe.write_all(&final_zip_content)?;

    // Cleanup
    let _ = fs::remove_file(temp_zip_path);

    println!("[5/5] Done!");
    println!("Created: {:?}", output_path);
    println!("Size: {} bytes", final_exe.metadata()?.len());

    if open {
        open_output(&output_path);
    }

    Ok(())
}

fn open_output(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn();
    }
}

fn find_project_root(self_dir: &Path, cwd: &Path) -> anyhow::Result<PathBuf> {
    let candidates = vec![
        self_dir.to_path_buf(),
        self_dir.parent().unwrap_or(Path::new("")).to_path_buf(),
        self_dir.parent().and_then(|p| p.parent()).unwrap_or(Path::new("")).to_path_buf(),
        cwd.to_path_buf(),
        cwd.join("Lunu"),
    ];

    for candidate in candidates {
        if candidate.join("init.luau").exists() && candidate.join("modules").exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "Could not locate Lunu project root (containing init.luau and modules)."
    ))
}

fn resolve_lune_path(project_root: &Path) -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("LUNE_PATH") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    if let Some(version) = lune_version_from_rokit(project_root) {
        if let Some(home) = dirs::home_dir() {
            let candidate = home
                .join(".rokit")
                .join("tool-storage")
                .join("lune-org")
                .join("lune")
                .join(version)
                .join("lune.exe");
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    if let Some(path) = find_in_path("lune.exe") {
        return Ok(path);
    }

    Err(anyhow::anyhow!(
        "Lune runtime not found. Set LUNE_PATH or ensure 'lune.exe' is in PATH."
    ))
}

fn lune_version_from_rokit(project_root: &Path) -> Option<String> {
    let rokit_path = project_root.join("rokit.toml");
    let content = std::fs::read_to_string(rokit_path).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("lune") {
            let parts: Vec<&str> = trimmed.split('=').collect();
            if parts.len() == 2 {
                let version = parts[1].trim().trim_matches('"');
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }
    }
    None
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

fn add_dir_to_zip<W: Write + io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    src_dir: &Path,
    prefix: &str,
    options: FileOptions,
) -> anyhow::Result<()> {
    if !src_dir.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(src_dir) {
        let entry = entry?;
        let path = entry.path();
        
        if path.is_dir() {
            continue;
        }

        let rel_path = path.strip_prefix(src_dir)?;
        let zip_path = Path::new(prefix).join(rel_path);
        let zip_path_str = zip_path.to_string_lossy().replace("\\", "/");

        zip.start_file(zip_path_str, options)?;
        let mut content = Vec::new();
        File::open(path)?.read_to_end(&mut content)?;
        zip.write_all(&content)?;
    }
    Ok(())
}
