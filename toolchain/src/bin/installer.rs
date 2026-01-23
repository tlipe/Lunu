use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{self, Write};
use anyhow::{Result, Context, anyhow};
use winreg::enums::*;
use winreg::RegKey;

fn main() -> Result<()> {
    println!("Lunu Installer v0.1.0");
    println!("=====================");

    let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
    let install_dir = home_dir.join(".lunu").join("bin");

    println!("Target: {:?}", install_dir);

    // 3. Create Install Directory
    if !install_dir.exists() {
        println!("Creating install directory...");
        fs::create_dir_all(&install_dir)?;
    }

    // 4. Extract Binaries
    println!("Extracting binaries...");
    
    // Embed binaries at compile time
    // Note: These paths are relative to the Cargo.toml location (Lunu/toolchain)
    // or relative to the source file depending on how macro is invoked.
    // include_bytes! path is relative to the current file.
    
    // We expect the binaries to be built in ../../target/release/ and ../../../builder/target/release/
    // This requires a multi-step build process in build.ps1

    let lunu_bytes = include_bytes!("../../target/release/lunu-cli.exe");
    let bridge_bytes = include_bytes!("../../target/release/lunu-bridge.exe");
    let builder_bytes = include_bytes!("../../../builder/target/release/lunu-build.exe");
    let stub_bytes = include_bytes!("../../../builder/target/release/lunu-stub.exe");

    write_binary(&install_dir.join("lunu.exe"), lunu_bytes)?;
    write_binary(&install_dir.join("lunu-bridge.exe"), bridge_bytes)?;
    write_binary(&install_dir.join("lunu-build.exe"), builder_bytes)?;
    write_binary(&install_dir.join("lunu-stub.exe"), stub_bytes)?;

    // 5. Setup PATH
    println!("Setting up PATH...");
    setup_path(&install_dir)?;

    println!("\nInstallation Successful! 🎉");
    println!("Please restart your terminal (or VS Code) for changes to take effect.");
    println!("Try running: lunu --help");

    // Pause before exit
    print!("\nPress Enter to exit...");
    io::stdout().flush()?;
    let _ = io::stdin().read_line(&mut String::new());

    Ok(())
}

fn write_binary(path: &Path, bytes: &[u8]) -> Result<()> {
    println!("  Extracting -> {:?}", path);
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
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
        
        // Notify system of environment change (Optional, but good practice)
        // Using minimal unsafe call or just skip for now as restart is recommended
        println!("  Added to User PATH.");
    } else {
        println!("  Already in PATH.");
    }

    Ok(())
}

#[cfg(not(windows))]
fn setup_path(bin_dir: &Path) -> Result<()> {
    println!("  Automatic PATH setup is only supported on Windows.");
    println!("  Please add the following to your shell profile (.bashrc, .zshrc, etc.):");
    println!("  export PATH=\"$PATH:{}\"", bin_dir.display());
    Ok(())
}
