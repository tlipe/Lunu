use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use walkdir::WalkDir;

// Embed the stub binary
const STUB_BYTES: &[u8] = include_bytes!("resources/lunu-stub.exe");

pub fn build_executable(
    script: &Path,
    output: Option<PathBuf>,
    force: bool,
    open: bool,
    icon: Option<PathBuf>,
    open_cmd: Option<bool>,
    custom_runtime_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    println!("Lunu Builder v0.1.2 (Internal)");
    println!("-------------------------------");

    if !cfg!(windows) {
        return Err(anyhow::anyhow!("Platform not supported for standalone build. Only Windows is supported in this build."));
    }

    if !script.exists() {
        return Err(anyhow::anyhow!("Input script not found: {:?}", script));
    }

    // 1. Determine Output Name
    let cwd = std::env::current_dir()?;
    let output_path = output.unwrap_or_else(|| {
        let stem = script.file_stem().unwrap_or_default();
        let mut p = cwd.join(stem); // Force CWD
        p.set_extension("exe");
        p
    });

    println!("[1/5] Target: {:?}", output_path);
    
    // 2. Dependencies
    let self_exe = std::env::current_exe()?;
    let self_dir = self_exe.parent().unwrap();
    
    // 3. Cache System
    let cache_dir = dirs::cache_dir().unwrap_or(cwd.join(".cache")).join("lunu-builder");
    fs::create_dir_all(&cache_dir)?;
    
    let cache_file = cache_dir.join("runtime_payload.zip");
    let cache_meta = cache_dir.join("runtime_payload.meta");
    let use_cache = !force && cache_file.exists();

    let mut base_zip_buffer = Vec::new();

    let mut cache_ok = false;
    if use_cache && cache_meta.exists() {
        let project_root = find_project_root(self_dir, &cwd)?;
        let lune_path = resolve_lune_path(&project_root)?;
        cache_ok = is_cache_valid(&cache_meta, &project_root, &lune_path)?;
    }

    if cache_ok {
        println!("[2/5] Loading runtime from cache...");
        let mut f = File::open(&cache_file)?;
        f.read_to_end(&mut base_zip_buffer)?;
    } else {
        println!("[2/5] Building runtime payload (this takes a moment)...");
        
        let project_root = find_project_root(self_dir, &cwd)?;
        let settings_path = project_root.join("config").join("settings.json");
        if !settings_path.exists() {
            return Err(anyhow::anyhow!("Config not found at {:?}. Run 'lunu init' in the project directory.", settings_path));
        }
        let lune_path = if let Some(p) = custom_runtime_path {
            p
        } else {
            resolve_lune_path(&project_root)?
        };
        if !lune_path.exists() {
            return Err(anyhow::anyhow!("Lune runtime not found at {:?}.", lune_path));
        }

        let mut zip_writer = zip::ZipWriter::new(std::io::Cursor::new(&mut base_zip_buffer));
        // Use Deflated (compression) instead of Stored to reduce binary size significantly
        let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip_writer.start_file("bin/lune.exe", options)?;
        let mut lune_content = Vec::new();
        File::open(&lune_path)?.read_to_end(&mut lune_content)?;
        zip_writer.write_all(&lune_content)?;

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

        let init_path = project_root.join("init.luau");
        if init_path.exists() {
            let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip_writer.start_file("Lunu/init.luau", options)?;
            let mut content = Vec::new();
            File::open(&init_path)?.read_to_end(&mut content)?;
            zip_writer.write_all(&content)?;

            zip_writer.start_file("init.luau", options)?;
            zip_writer.write_all(&content)?;
        }

        let luaurc_path = project_root.join(".luaurc");
        if luaurc_path.exists() {
            let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip_writer.start_file(".luaurc", options)?;
            let mut content = Vec::new();
            File::open(&luaurc_path)?.read_to_end(&mut content)?;
            zip_writer.write_all(&content)?;
        }

        zip_writer.finish()?;
        drop(zip_writer);
        
        let mut f = File::create(&cache_file)?;
        f.write_all(&base_zip_buffer)?;
        let meta = build_cache_meta(&project_root, &lune_path)?;
        let mut mf = File::create(&cache_meta)?;
        mf.write_all(meta.as_bytes())?;
    }

    println!("[3/5] Injecting user script...");
    
    let temp_zip_path = temp_zip_path();
    fs::copy(&cache_file, &temp_zip_path)?;
    
    let file = fs::OpenOptions::new().read(true).write(true).open(&temp_zip_path)?;
    let mut zip_writer = zip::ZipWriter::new_append(file)?;
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

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

    // 4. Concatenate Embedded Stub + Final Zip
    let mut final_exe = File::create(&output_path)?;
    
    // Use embedded bytes
    final_exe.write_all(STUB_BYTES)?;
    
    let mut final_zip_content = Vec::new();
    File::open(&temp_zip_path)?.read_to_end(&mut final_zip_content)?;
    final_exe.write_all(&final_zip_content)?;

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
        cwd.to_path_buf(),
        cwd.join("Lunu"),
        self_dir.to_path_buf(),
        self_dir.parent().unwrap_or(Path::new("")).to_path_buf(),
        self_dir.parent().and_then(|p| p.parent()).unwrap_or(Path::new("")).to_path_buf(),
    ];

    for candidate in candidates {
        let has_modules = candidate.join("modules").exists();
        let has_manifest = candidate.join("lunu.toml").exists();
        let has_settings = candidate.join("config").join("settings.json").exists();
        let has_entry = candidate.join("src").join("main.luau").exists();
        if has_modules && (has_manifest || has_settings || has_entry) {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "Could not locate Lunu project root (missing lunu.toml, config/settings.json, or src/main.luau)."
    ))
}

fn resolve_lune_path(project_root: &Path) -> anyhow::Result<PathBuf> {
    let project_bin = project_root.join("bin").join("lune.exe");
    if is_runtime_candidate(&project_bin) {
        return Ok(project_bin);
    }

    if let Ok(path) = std::env::var("LUNE_PATH") {
        let p = PathBuf::from(path);
        if is_runtime_candidate(&p) {
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
            if is_runtime_candidate(&candidate) {
                return Ok(candidate);
            }
        }
    }

    if let Some(candidate) = find_rokit_tool_storage_lune() {
        return Ok(candidate);
    }

    if let Some(path) = find_in_path("lune.exe") {
        if is_runtime_candidate(&path) {
            return Ok(path);
        }
    }

    Err(anyhow::anyhow!(
        "Lune runtime not found. Set LUNE_PATH, place bin/lune.exe in the project, or install via rokit."
    ))
}

fn is_runtime_candidate(path: &Path) -> bool {
    if is_rokit_shim(path) {
        return false;
    }
    std::fs::metadata(path).map(|m| m.len() >= 200_000).unwrap_or(false)
}

fn is_rokit_shim(path: &Path) -> bool {
    let mut has_rokit = false;
    let mut has_tool_storage = false;
    let mut has_bin_or_shims = false;
    for comp in path.components() {
        let part = comp.as_os_str().to_string_lossy().to_lowercase();
        if part == ".rokit" || part == "rokit" {
            has_rokit = true;
        }
        if part == "tool-storage" {
            has_tool_storage = true;
        }
        if part == "bin" || part == "shims" {
            has_bin_or_shims = true;
        }
    }
    has_rokit && has_bin_or_shims && !has_tool_storage
}

fn find_rokit_tool_storage_lune() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let base = home
        .join(".rokit")
        .join("tool-storage")
        .join("lune-org")
        .join("lune");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(&base).ok()?;
    for entry in entries.flatten() {
        let candidate = entry.path().join("lune.exe");
        if !is_runtime_candidate(&candidate) {
            continue;
        }
        let mtime = std::fs::metadata(&candidate)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        match &best {
            Some((best_time, _)) if *best_time >= mtime => {}
            _ => {
                best = Some((mtime, candidate));
            }
        }
    }
    best.map(|(_, path)| path)
}
fn build_cache_meta(project_root: &Path, lune_path: &Path) -> anyhow::Result<String> {
    let lune_meta = std::fs::metadata(lune_path)?;
    let lune_mtime = lune_meta.modified().ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let lune_size = lune_meta.len();
    Ok(format!("root={}\nlune_path={}\nlune_mtime={}\nlune_size={}\n", project_root.display(), lune_path.display(), lune_mtime, lune_size))
}

fn is_cache_valid(cache_meta: &Path, project_root: &Path, lune_path: &Path) -> anyhow::Result<bool> {
    let current = build_cache_meta(project_root, lune_path)?;
    let saved = std::fs::read_to_string(cache_meta).unwrap_or_default();
    Ok(current == saved)
}

fn temp_zip_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("lunu_build_{}_{}.zip", pid, nanos))
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

fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    src_dir: &Path,
    dst_dir: &str,
    options: FileOptions,
) -> anyhow::Result<()> {
    if !src_dir.exists() {
        return Ok(());
    }

    for entry in WalkDir::new(src_dir) {
        let entry = entry?;
        let path = entry.path();
        
        // Filter out unnecessary files
        if path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == "target" || s == ".git" || s == "node_modules" || s == "__pycache__"
        }) {
            continue;
        }

        let name = path.strip_prefix(src_dir)?;
        let path_str = name.to_string_lossy().replace("\\", "/");
        let zip_path = format!("{}/{}", dst_dir, path_str);

        if path.is_file() {
            zip.start_file(zip_path, options)?;
            let mut f = File::open(path)?;
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer)?;
            zip.write_all(&buffer)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;

    fn temp_test_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lunu_builder_test_{}_{}", std::process::id(), nanos));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn lune_version_from_rokit_parses_value() {
        let root = temp_test_dir();
        std::fs::write(root.join("rokit.toml"), "[tools]\nlune = \"0.10.4\"\n").unwrap();
        let version = lune_version_from_rokit(&root).unwrap();
        assert_eq!(version, "0.10.4");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn temp_zip_path_is_unique() {
        let a = temp_zip_path();
        let b = temp_zip_path();
        assert_ne!(a, b);
    }

    fn write_file_with_size(path: &Path, size: u64) {
        let mut file = OpenOptions::new().create(true).write(true).open(path).unwrap();
        file.write_all(b"x").unwrap();
        file.set_len(size).unwrap();
    }

    #[test]
    fn resolve_lune_path_prefers_project_bin() {
        let root = temp_test_dir();
        let project_bin = root.join("bin");
        std::fs::create_dir_all(&project_bin).unwrap();
        let project_lune = project_bin.join("lune.exe");
        write_file_with_size(&project_lune, 300_000);

        let alt = root.join("alt_lune.exe");
        write_file_with_size(&alt, 400_000);
        std::env::set_var("LUNE_PATH", &alt);

        let found = resolve_lune_path(&root).unwrap();
        assert_eq!(found, project_lune);

        std::env::remove_var("LUNE_PATH");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resolve_lune_path_ignores_small_shim() {
        let root = temp_test_dir();
        let shim = root.join("shim_lune.exe");
        write_file_with_size(&shim, 10_000);
        std::env::set_var("LUNE_PATH", &shim);

        let home = root.join("home");
        let rokit_lune = home
            .join(".rokit")
            .join("tool-storage")
            .join("lune-org")
            .join("lune")
            .join("0.10.4")
            .join("lune.exe");
        std::fs::create_dir_all(rokit_lune.parent().unwrap()).unwrap();
        write_file_with_size(&rokit_lune, 300_000);
        std::fs::write(root.join("rokit.toml"), "[tools]\nlune = \"0.10.4\"\n").unwrap();

        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("HOMEDRIVE", "C:");
        std::env::set_var("HOMEPATH", "\\Users\\TempUser");
        let found = resolve_lune_path(&root).unwrap();
        assert_ne!(found, shim);
        assert!(is_runtime_candidate(&found));

        std::env::remove_var("LUNE_PATH");
        std::env::remove_var("USERPROFILE");
        std::env::remove_var("HOMEDRIVE");
        std::env::remove_var("HOMEPATH");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn is_rokit_shim_detects_rokit_bin() {
        let shim = PathBuf::from("C:\\Users\\User\\.rokit\\bin\\lune.exe");
        assert!(is_rokit_shim(&shim));
        let runtime = PathBuf::from("C:\\Users\\User\\.rokit\\tool-storage\\lune-org\\lune\\0.10.4\\lune.exe");
        assert!(!is_rokit_shim(&runtime));
    }
}
