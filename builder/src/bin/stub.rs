
use std::env;
use std::fs::{self, File};
use std::io::{self, copy};
use std::process::Command;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use zip::ZipArchive;

static OPEN_CMD: AtomicBool = AtomicBool::new(true);

// Minimal Error type to avoid anyhow overhead
type StubResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() {
    // Custom panic hook to keep window open on error if needed
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
        if OPEN_CMD.load(Ordering::Relaxed) {
            println!("\nExecution finished (Error). Press Enter to exit...");
            let mut buffer = String::new();
            let _ = std::io::stdin().read_line(&mut buffer);
        }
    } else {
        if OPEN_CMD.load(Ordering::Relaxed) {
            // println!("\nExecution finished. Press Enter to exit...");
            // Let's be silent on success unless requested? 
            // Original code waited. Let's keep waiting.
            // Actually, for a production stub, we usually don't want to wait on success.
            // But let's respect the flag.
             let mut buffer = String::new();
             let _ = std::io::stdin().read_line(&mut buffer);
        }
    }
}

fn run() -> StubResult<()> {
    let exe_path = env::current_exe()?;
    let file = File::open(&exe_path)?;
    
    // Try to open ZIP
    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => {
            eprintln!("[Lunu Stub] No embedded content found.");
            return Ok(());
        }
    };

    // Create temp dir manually to avoid tempfile dependency
    let temp_base = env::temp_dir();
    let unique_name = format!("lunu_{}", std::process::id()); // Simple unique ID
    let root = temp_base.join(unique_name);
    
    // Cleanup previous run if exists
    if root.exists() {
        let _ = fs::remove_dir_all(&root);
    }
    fs::create_dir_all(&root)?;

    // Extract
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
            copy(&mut file, &mut outfile)?;
        }
    }

    // Locate Lune
    let lune_exe = root.join("bin").join("lune.exe");
    let main_script = root.join("src").join("main.luau");
    
    // Check open_cmd flag
    let open_cmd = read_open_cmd(&root);
    OPEN_CMD.store(open_cmd, Ordering::Relaxed);

    if !lune_exe.exists() {
        eprintln!("[Lunu Stub] Critical: Runtime (lune.exe) not found.");
        return Ok(());
    }

    // Run Lune
    let mut cmd = Command::new(&lune_exe);
    cmd.arg("run")
       .arg(&main_script)
       .current_dir(&root);
       
    // Forward args? The stub might receive args.
    // For now, simple run.
    
    let mut child = cmd.spawn()?;
    let status = child.wait()?;

    if !status.success() {
        eprintln!("\n[Lunu Stub] Script exited with code: {:?}", status.code());
    }

    // Cleanup Temp?
    // In a real optimized stub, we might leave it or delete it.
    // Deleting is good.
    let _ = fs::remove_dir_all(&root);

    Ok(())
}

fn read_open_cmd(root: &Path) -> bool {
    let flag_path = root.join("lunu_open_cmd.txt");
    if let Ok(content) = fs::read_to_string(flag_path) {
        let value = content.trim();
        if value.eq_ignore_ascii_case("0") || value.eq_ignore_ascii_case("false") || value.eq_ignore_ascii_case("no") {
            return false;
        }
    }
    true
}
