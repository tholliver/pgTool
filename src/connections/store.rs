use anyhow::Result;
use std::path::PathBuf;

use super::profile::ConnectionProfile;

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pgTool")
}

pub fn connections_path() -> PathBuf {
    config_dir().join("connections.json")
}

pub fn save_profiles(profiles: &[ConnectionProfile]) -> Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_string_pretty(profiles)?;
    std::fs::write(connections_path(), json)?;
    Ok(())
}

pub fn load_profiles() -> Result<Vec<ConnectionProfile>> {
    let path = connections_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)?;
    let profiles: Vec<ConnectionProfile> = serde_json::from_str(&data)?;
    Ok(profiles)
}
