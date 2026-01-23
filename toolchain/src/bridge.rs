use std::process::{Command, Stdio};
use std::path::PathBuf;
use anyhow::{Result, anyhow};

pub struct BridgeManager {
    lunu_root: PathBuf,
}

impl BridgeManager {
    pub fn new(workspace_root: PathBuf) -> Self {
        // The toolchain expects to find the 'Lunu' directory inside the workspace root
        // If workspace_root ends with "Lunu" (e.g. user ran from Lunu dir), use it directly
        if workspace_root.ends_with("Lunu") {
            Self { lunu_root: workspace_root }
        } else {
            Self { lunu_root: workspace_root.join("Lunu") }
        }
    }

    pub fn start(&self, daemon: bool) -> Result<()> {
        let bridge_exe = find_bridge_exe(&self.lunu_root)?;

        if !daemon {
            println!("Starting Lunu Bridge Server...");
            println!("   Executable: {:?}", bridge_exe);
            println!("   Mode: Foreground (Press Ctrl+C to stop)");
        }

        let mut cmd = Command::new(&bridge_exe);
        cmd.current_dir(&self.lunu_root); // Critical: Server expects to run from Lunu root

        if daemon {
            // Windows specific detachment to ensure it survives parent exit and has no window
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // CREATE_NO_WINDOW = 0x08000000
                // DETACHED_PROCESS = 0x00000008
                cmd.creation_flags(0x08000000); 
            }
            
            // Redirect stdio to null to avoid hanging parent
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
            cmd.stdin(Stdio::null());

            cmd.spawn().map_err(|e| anyhow!("Failed to spawn background process: {}", e))?;
            println!("Lunu Bridge Server started in background. 🚀");
        } else {
            // Blocking mode - inherit stdio so user sees logs
            let mut child = cmd.spawn()?;
            let status = child.wait()?;
            if !status.success() {
                return Err(anyhow!("Server exited with error code: {:?}", status.code()));
            }
        }

        Ok(())
    }
}

fn find_bridge_exe(lunu_root: &PathBuf) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(lunu_root.join("bin").join("lunu-bridge.exe"));
    candidates.push(lunu_root.join("lunu-bridge.exe"));
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            candidates.push(dir.join("lunu-bridge.exe"));
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow!("Lunu Bridge executable not found. Please run toolchain build to generate lunu-bridge.exe."))
}
