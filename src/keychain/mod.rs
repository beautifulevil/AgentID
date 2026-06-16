use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_token: String,
    pub refresh_token: String,
    pub device_id: String,
    pub user_email: String,
    pub expires_at: String,
}

static SESSION_CACHE: Mutex<Option<Result<Session, String>>> = Mutex::new(None);

pub fn save_session(session: &Session) -> Result<()> {
    let path = session_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(session)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    *SESSION_CACHE.lock().expect("session cache lock") = None;
    Ok(())
}

pub fn load_session() -> Result<Session> {
    let mut cache = SESSION_CACHE.lock().expect("session cache lock");
    if let Some(stored) = cache.as_ref() {
        return stored.clone().map_err(|message| anyhow!("{message}"));
    }
    let loaded = load_session_from_disk();
    *cache = Some(
        loaded
            .as_ref()
            .map(|session| session.clone())
            .map_err(|error| error.to_string()),
    );
    loaded
}

pub fn delete_session() -> Result<()> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("failed to delete {}", path.display()))?;
    }
    *SESSION_CACHE.lock().expect("session cache lock") = None;
    Ok(())
}

fn load_session_from_disk() -> Result<Session> {
    let path = session_path()?;
    if !path.exists() {
        return Err(anyhow!("not logged in; run `agentid login`"));
    }
    let value =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&value).context("stored AgentID session is invalid")
}

fn session_path() -> Result<PathBuf> {
    let base = dirs::config_dir().ok_or_else(|| anyhow!("could not determine config directory"))?;
    Ok(base.join("agentid").join("session.json"))
}
