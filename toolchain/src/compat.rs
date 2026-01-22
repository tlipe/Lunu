use std::path::{Path, PathBuf};
use anyhow::Result;
use tokio::fs;

pub struct CompatibilityLayer;

impl CompatibilityLayer {
    pub async fn ensure_compat(path: &Path) -> Result<()> {
        println!("Running compatibility check on {:?}", path);

        // 1. Check for entry point
        let init_luau = path.join("init.luau");
        let init_lua = path.join("init.lua");

        if !init_luau.exists() && !init_lua.exists() {
            println!("No init file found. Generating wrapper...");
            Self::generate_wrapper(path).await?;
        }

        // 2. Check dependencies (Mock implementation for Wally/Rokit)
        if path.join("wally.toml").exists() {
            println!("Detected Wally project. Resolving dependencies is not yet fully supported, but files are intact.");
        }

        Self::ensure_manifest(path).await?;

        Ok(())
    }

    async fn generate_wrapper(path: &Path) -> Result<()> {
        // Simple heuristic: expose all .luau files as fields in a table
        let mut export_lines = Vec::new();
        export_lines.push("return {".to_string());

        let mut read_dir = fs::read_dir(path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let p = entry.path();
            if let Some(ext) = p.extension() {
                if ext == "luau" || ext == "lua" {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        export_lines.push(format!("    {} = require(\"./{}\"),", stem, stem));
                    }
                }
            }
        }

        export_lines.push("}".to_string());
        
        fs::write(path.join("init.luau"), export_lines.join("\n")).await?;
        Ok(())
    }

    async fn ensure_manifest(path: &Path) -> Result<()> {
        let manifest_path = path.join("lunu.toml");
        if manifest_path.exists() {
            return Ok(());
        }

        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("lunu-lib");

        let entry = if path.join("init.luau").exists() {
            "init.luau"
        } else if path.join("init.lua").exists() {
            "init.lua"
        } else {
            "init.luau"
        };

        let content = format!(
            "name = \"{}\"\nversion = \"0.1.0\"\nentry = \"{}\"\nlanguage = \"luau\"\n",
            name, entry
        );

        fs::write(manifest_path, content).await?;
        Ok(())
    }
}
