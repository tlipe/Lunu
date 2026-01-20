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
}
