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
        let python_path = self.lunu_root.join(".venv").join("Scripts").join("python.exe");
        let server_script = self.lunu_root.join("src").join("bridge").join("server.py");

        if !python_path.exists() {
            return Err(anyhow!("Python environment not found at {:?}.\nPlease run 'scripts/setup.ps1' inside Lunu folder.", python_path));
        }

        if !daemon {
            println!("Starting Lunu Bridge Server...");
            println!("   Python: {:?}", python_path);
            println!("   Script: {:?}", server_script);
            println!("   Mode: Foreground (Press Ctrl+C to stop)");
        }

        let mut cmd = Command::new(&python_path);
        cmd.arg(&server_script);
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
