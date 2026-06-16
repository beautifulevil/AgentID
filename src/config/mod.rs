use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalConfig {
    pub api_base_url: Option<String>,
    pub selected_org: Option<String>,
    pub workspace_path: Option<PathBuf>,
    #[serde(default)]
    pub known_workspaces: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub org: String,
    pub workspace_path: PathBuf,
    pub active_agent: Option<String>,
    #[serde(default)]
    pub detected_repos: Vec<String>,
}

pub fn load_global() -> Result<GlobalConfig> {
    let path = global_config_path()?;
    if !path.exists() {
        return Ok(GlobalConfig::default());
    }
    let data =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn save_global(config: &GlobalConfig) -> Result<()> {
    let path = global_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(config)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn init_workspace(path: &Path, org: String) -> Result<WorkspaceConfig> {
    let absolute = expand_path(path)?;
    fs::create_dir_all(absolute.join(".agentid"))?;
    let config = WorkspaceConfig {
        org,
        workspace_path: absolute.clone(),
        active_agent: None,
        detected_repos: Vec::new(),
    };
    save_workspace(&config)?;
    register_known_workspace(&absolute)?;
    Ok(config)
}

pub fn save_workspace(config: &WorkspaceConfig) -> Result<()> {
    let path = workspace_config_path(&config.workspace_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_yaml::to_string(config)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn load_workspace() -> Result<WorkspaceConfig> {
    if let Some(path) = find_workspace_config()? {
        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        return serde_yaml::from_str(&data)
            .with_context(|| format!("failed to parse {}", path.display()));
    }

    let global = load_global()?;
    if let Some(path) = global.workspace_path {
        let config_path = workspace_config_path(&path);
        if config_path.exists() {
            let data = fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            return serde_yaml::from_str(&data)
                .with_context(|| format!("failed to parse {}", config_path.display()));
        }
    }

    Err(anyhow!(
        "workspace is not initialized; run `agentid workspace init <path>` first"
    ))
}

pub fn workspace_exists(path: &Path) -> Result<bool> {
    let absolute = expand_path(path)?;
    Ok(workspace_config_path(&absolute).exists())
}

pub fn workspace_config_path(workspace_path: &Path) -> PathBuf {
    workspace_path.join(".agentid").join("workspace.yaml")
}

pub fn load_workspace_at(workspace_path: &Path) -> Result<WorkspaceConfig> {
    let path = workspace_config_path(workspace_path);
    if !path.exists() {
        return Err(anyhow!(
            "no workspace at {} — run `agentid init` first",
            workspace_path.display()
        ));
    }
    let data =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn register_known_workspace(workspace_path: &Path) -> Result<()> {
    let absolute = expand_path(workspace_path)?;
    let mut global = load_global().unwrap_or_default();
    if !global.known_workspaces.iter().any(|path| path == &absolute) {
        global.known_workspaces.push(absolute);
        save_global(&global)?;
    }
    Ok(())
}

pub fn list_known_workspaces() -> Result<Vec<WorkspaceConfig>> {
    let mut global = load_global().unwrap_or_default();
    let mut paths: Vec<PathBuf> = global.known_workspaces.clone();
    if let Some(path) = global.workspace_path.clone() {
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }
    if let Some(current) = find_workspace_config()? {
        if let Some(parent) = current.parent().and_then(|dir| dir.parent()) {
            if !paths.iter().any(|existing| existing == parent) {
                paths.push(parent.to_path_buf());
            }
        }
    }

    let mut workspaces = Vec::new();
    let mut seen = Vec::new();
    for path in paths {
        if seen.iter().any(|existing: &PathBuf| existing == &path) {
            continue;
        }
        seen.push(path.clone());
        if let Ok(workspace) = load_workspace_at(&path) {
            workspaces.push(workspace);
        }
    }

    global.known_workspaces = workspaces
        .iter()
        .map(|workspace| workspace.workspace_path.clone())
        .collect();
    save_global(&global)?;
    Ok(workspaces)
}

pub fn delete_workspace(workspace_path: &Path) -> Result<()> {
    let absolute = expand_path(workspace_path)?;
    let config_path = workspace_config_path(&absolute);
    if config_path.exists() {
        fs::remove_file(&config_path)
            .with_context(|| format!("failed to remove {}", config_path.display()))?;
    }
    let agentid_dir = absolute.join(".agentid");
    if agentid_dir.is_dir() {
        let remaining = fs::read_dir(&agentid_dir)?.count();
        if remaining == 0 {
            fs::remove_dir(&agentid_dir)?;
        }
    }

    let mut global = load_global().unwrap_or_default();
    global.known_workspaces.retain(|path| path != &absolute);
    if global.workspace_path.as_ref() == Some(&absolute) {
        global.workspace_path = None;
    }
    save_global(&global)?;
    Ok(())
}

fn global_config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("could not determine config directory"))?;
    Ok(base.join("agentid").join("config.json"))
}

fn find_workspace_config() -> Result<Option<PathBuf>> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = workspace_config_path(&dir);
        if candidate.exists() {
            return Ok(Some(candidate));
        }
        if !dir.pop() {
            return Ok(None);
        }
    }
}

fn expand_path(path: &Path) -> Result<PathBuf> {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join(stripped));
        }
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
