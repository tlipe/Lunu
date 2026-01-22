use std::env;
use std::fs::{self, File};
use std::io::{self};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use zip::ZipArchive;
use tempfile::Builder;

static OPEN_CMD: AtomicBool = AtomicBool::new(true);

fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("\n[Lunu Stub] CRITICAL PANIC: {}", info);
        if OPEN_CMD.load(Ordering::Relaxed) {
            eprintln!("Press Enter to exit...");
            let mut buffer = String::new();
            let _ = std::io::stdin().read_line(&mut buffer);
        }
    }));

    if let Err(e) = run() {
        eprintln!("\n[Lunu Stub] Fatal Error: {}", e);
    }

    if OPEN_CMD.load(Ordering::Relaxed) {
        println!("\nExecution finished. Press Enter to exit...");
        let mut buffer = String::new();
        let _ = std::io::stdin().read_line(&mut buffer);
    }
}

fn run() -> anyhow::Result<()> {
    // 1. Locate ourselves (the executable)
    let exe_path = env::current_exe()?;
    
    // 2. Open ourselves to find the attached ZIP
    // Strategy: Look for a magic footer or just try to open as ZIP.
    // Since we append the ZIP to the end, standard ZIP readers might read from end.
    // The 'zip' crate supports reading from a file, it usually looks for the central directory at the end.
    
    let file = File::open(&exe_path)?;
    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            eprintln!("[Lunu Stub] Error: No embedded content found. This is a raw stub.");
            return Ok(());
        }
    };

    // 3. Create a temporary directory for this run
    // We use a stable temp dir based on UUID or hash so we don't spam temp if we reuse it?
    // For now, fresh temp dir every time to avoid conflicts.
    let temp_dir = Builder::new().prefix("lunu_app_").tempdir()?;
    let root = temp_dir.path();
    // println!("[Lunu Stub] Extracting to {:?}", root);

    // 4. Extract everything
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(path) => root.join(path),
            None => continue,
        };

        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }

    // 5. Locate Lune and Main Script
    // Structure expected inside ZIP:
    // /bin/lune.exe
    // /src/main.luau
    // /daemon/ (optional)
    
    let lune_exe = root.join("bin").join("lune.exe");
    let main_script = root.join("src").join("main.luau");
    let open_cmd = read_open_cmd(root);
    OPEN_CMD.store(open_cmd, Ordering::Relaxed);

    if !lune_exe.exists() {
        eprintln!("[Lunu Stub] Critical: Runtime (lune.exe) not found in package.");
        return Ok(());
    }
    
    // 7. Run Lune
    let mut child = Command::new(&lune_exe)
        .arg("run")
        .arg(&main_script)
        .current_dir(root) // Run inside the temp environment so relative requires work
        .spawn()?;

    let status = child.wait()?;

    if !status.success() {
        eprintln!("\n[Lunu Stub] Script exited with code: {:?}", status.code());
    }

    // Temp dir is auto-deleted when 'temp_dir' goes out of scope.
    Ok(())
}

fn read_open_cmd(root: &std::path::Path) -> bool {
    let flag_path = root.join("lunu_open_cmd.txt");
    if let Ok(content) = fs::read_to_string(flag_path) {
        let value = content.trim();
        if value.eq_ignore_ascii_case("0") || value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("no") {
            return false;
        }
    }
    true
}
