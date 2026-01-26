use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Luaurc {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
    #[serde(flatten)]
    pub other: BTreeMap<String, serde_json::Value>,
}

impl Luaurc {
    pub async fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                aliases: BTreeMap::new(),
                other: BTreeMap::new(),
            });
        }
        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read .luaurc at {:?}", path))?;
        
        let config: Luaurc = serde_json::from_str(&content)
            .with_context(|| "Failed to parse .luaurc")?;
            
        Ok(config)
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .with_context(|| "Failed to serialize .luaurc")?;
        
        fs::write(path, content).await
            .with_context(|| format!("Failed to write .luaurc to {:?}", path))?;
        
        Ok(())
    }

    pub fn add_alias(&mut self, name: &str, path: &str) {
        self.aliases.insert(name.to_string(), path.to_string());
    }

    pub fn remove_alias(&mut self, name: &str) {
        self.aliases.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn load_missing_returns_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".luaurc");

        let config = Luaurc::load(&path).await.unwrap();
        assert!(config.aliases.is_empty());
        assert!(config.other.is_empty());
    }

    #[tokio::test]
    async fn save_and_reload_aliases() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".luaurc");

        let mut config = Luaurc {
            aliases: BTreeMap::new(),
            other: BTreeMap::new(),
        };
        config.add_alias("lunu", "modules/lunu/");
        config.save(&path).await.unwrap();

        let loaded = Luaurc::load(&path).await.unwrap();
        assert_eq!(loaded.aliases.get("lunu").unwrap(), "modules/lunu/");
    }
}
