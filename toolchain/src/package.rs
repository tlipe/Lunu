use std::path::{Path, PathBuf};
use git2::{Repository, FetchOptions, build::RepoBuilder};
use anyhow::{Result, Context, anyhow};
use tokio::fs;
use sha2::{Sha256, Digest};
use path_clean::PathClean;

pub struct PackageManager {
    root_dir: PathBuf,
}

impl PackageManager {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    pub async fn install_package(&self, url: &str, version: Option<&str>, target_name: &str) -> Result<(PathBuf, String)> {
        // 1. Prepare Paths
        let install_path = self.root_dir.join("modules").join(target_name).clean();
        
        // 2. Clean existing
        if install_path.exists() {
            println!("Cleaning existing module at {:?}", install_path);
            fs::remove_dir_all(&install_path).await?;
        }
        fs::create_dir_all(&install_path).await?;

        // 3. Git Clone (Shallow)
        println!("Cloning {} to {:?}...", url, install_path);
        
        // Run blocking git operation in spawn_blocking
        let url_owned = url.to_string();
        let path_owned = install_path.clone();
        
        let _repo = tokio::task::spawn_blocking(move || {
            let mut fetch_opts = FetchOptions::new();
            fetch_opts.depth(1); // Shallow clone

            let mut builder = RepoBuilder::new();
            builder.fetch_options(fetch_opts);
            
            builder.clone(&url_owned, &path_owned)
        }).await??;

        // 4. Calculate Checksum
        let checksum = self.calculate_dir_checksum(&install_path).await?;

        // 5. Cleanup .venv if exists in the new package (Requisito 3.2)
        // Also cleanup global .venv if requested by user logic, but here we clean package specific artifacts
        let venv_path = install_path.join(".venv");
        if venv_path.exists() {
            println!("Removing conflicting .venv in package...");
            fs::remove_dir_all(venv_path).await.ok();
        }

        Ok((install_path, checksum))
    }

    pub async fn clean_global_venv(&self) -> Result<()> {
        let venv = self.root_dir.join(".venv");
        if venv.exists() {
            println!("Cleaning global .venv at {:?}", venv);
            fs::remove_dir_all(venv).await?;
        }
        Ok(())
    }

    pub async fn remove_package(&self, name: &str) -> Result<()> {
        let install_path = self.root_dir.join("modules").join(name).clean();
        if install_path.exists() {
            fs::remove_dir_all(&install_path).await?;
        }
        Ok(())
    }

    pub async fn calculate_dir_checksum(&self, path: &Path) -> Result<String> {
        let mut hasher = Sha256::new();
        let mut entries = Vec::new();

        let mut read_dir = fs::read_dir(path).await?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                entries.push(path);
            }
        }
        entries.sort();

        for file_path in entries {
            let bytes = fs::read(&file_path).await?;
            hasher.update(&bytes);
        }

        Ok(hex::encode(hasher.finalize()))
    }
}
