use std::collections::BTreeMap;
use std::path::Path;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub entry: String,
    pub modules_dir: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DependencySpec {
    pub url: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectConfig {
    pub project: ProjectInfo,
    #[serde(default)]
    pub dependencies: BTreeMap<String, DependencySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RuntimeConfig {
    pub name: String,
    pub security: String,
    pub performance: String,
    pub notes: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BuildConfig {
    pub kind: String,
    pub link: String,
    pub modules: String,
    #[serde(default)]
    pub module_languages: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    pub c_compiler: Option<String>,
    pub cpp_compiler: Option<String>,
    pub toolchain: Option<String>,
}

impl ProjectConfig {
    #[cfg(test)]
    pub fn new(name: &str) -> Self {
        Self {
            project: ProjectInfo {
                name: name.to_string(),
                entry: "src/main.luau".to_string(),
                modules_dir: "modules".to_string(),
            },
            dependencies: BTreeMap::new(),
            runtime: None,
            build: None,
        }
    }

    pub fn new_with_runtime(name: &str, runtime: RuntimeConfig, build: Option<BuildConfig>) -> Self {
        Self {
            project: ProjectInfo {
                name: name.to_string(),
                entry: "src/main.luau".to_string(),
                modules_dir: "modules".to_string(),
            },
            dependencies: BTreeMap::new(),
            runtime: Some(runtime),
            build,
        }
    }

    pub async fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).await
            .with_context(|| format!("Failed to read project config at {:?}", path))?;
        let cfg: ProjectConfig = toml::from_str(&content)
            .with_context(|| "Failed to parse lunu.toml")?;
        Ok(cfg)
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .with_context(|| "Failed to serialize lunu.toml")?;
        fs::write(path, content).await
            .with_context(|| format!("Failed to write lunu.toml to {:?}", path))?;
        Ok(())
    }

    pub fn add_dependency(&mut self, name: &str, spec: DependencySpec) {
        self.dependencies.insert(name.to_string(), spec);
    }

    pub fn remove_dependency(&mut self, name: &str) {
        self.dependencies.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn project_config_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lunu.toml");

        let mut cfg = ProjectConfig::new("TestProject");
        let mut dep = DependencySpec::default();
        dep.url = Some("https://github.com/example/repo".to_string());
        cfg.add_dependency("example", dep);
        cfg.save(&path).await.unwrap();

        let loaded = ProjectConfig::load(&path).await.unwrap();
        assert_eq!(loaded.project.name, "TestProject");
        assert!(loaded.dependencies.contains_key("example"));
    }
}
