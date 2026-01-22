use std::collections::BTreeMap;
use std::path::Path;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LockEntry {
    pub url: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
    pub checksum: String,
    pub installed_at: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct LockFile {
    #[serde(default)]
    pub dependencies: BTreeMap<String, LockEntry>,
}

impl LockFile {
    pub async fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read lunu.lock at {:?}", path))?;
        let lock: LockFile = toml::from_str(&content)
            .with_context(|| "Failed to parse lunu.lock")?;
        Ok(lock)
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize lunu.lock")?;
        fs::write(path, content).await
            .with_context(|| format!("Failed to write lunu.lock to {:?}", path))?;
        Ok(())
    }

    pub fn set(&mut self, name: &str, entry: LockEntry) {
        self.dependencies.insert(name.to_string(), entry);
    }

    pub fn remove(&mut self, name: &str) {
        self.dependencies.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn lockfile_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lunu.lock");
        let mut lock = LockFile::default();
        lock.set("example", LockEntry {
            url: Some("https://github.com/example/repo".to_string()),
            version: None,
            path: Some("modules/example".to_string()),
            checksum: "abc123".to_string(),
            installed_at: 1,
        });
        lock.save(&path).await.unwrap();

        let loaded = LockFile::load(&path).await.unwrap();
        assert!(loaded.dependencies.contains_key("example"));
    }
}
