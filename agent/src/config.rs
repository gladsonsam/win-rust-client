//! Persistent configuration and shared runtime state for the agent.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// ─── Configuration ────────────────────────────────────────────────────────────

/// Agent connection + security configuration, persisted to disk as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Full WebSocket URL of the Sentinel server.
    /// Example: `ws://192.168.1.100:9000/ws/agent`
    #[serde(default)]
    pub server_url: String,

    /// Friendly name sent to the server as `?name=<agent_name>`.
    /// Defaults to the Windows `COMPUTERNAME` environment variable.
    #[serde(default = "default_agent_name")]
    pub agent_name: String,

    /// Password / token forwarded to the server as `secret=...` for agent auth.
    #[serde(default)]
    pub agent_password: String,

    /// SHA-256 hex-digest of the local UI access password.
    /// An empty-string hash (`hash_password("")`) means no password required.
    #[serde(default = "empty_password_hash")]
    pub ui_password_hash: String,
}

fn default_agent_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "agent".into())
}

fn empty_password_hash() -> String {
    hash_password("")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: String::new(),
            agent_name: default_agent_name(),
            agent_password: String::new(),
            ui_password_hash: empty_password_hash(),
        }
    }
}

/// Returns the SHA-256 hex-digest of `password`.
pub fn hash_password(password: &str) -> String {
    let mut h = Sha256::new();
    h.update(password.as_bytes());
    format!("{:x}", h.finalize())
}

/// Path to the JSON config file on disk.
///
/// On Windows: `%LOCALAPPDATA%\sentinel\config.json`
pub fn config_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sentinel")
        .join("config.json")
}

/// Load configuration from disk; falls back to `Config::default()` on any error.
pub fn load_config() -> Config {
    let path = config_path();
    let mut cfg = if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str::<Config>(&data).unwrap_or_default()
    } else {
        Config::default()
    };

    // Optional environment overrides, useful when running headless (no UI).
    // These only override when the env var is present and non-empty.
    if let Ok(v) = std::env::var("AGENT_SERVER_URL") {
        let v = v.trim();
        if !v.is_empty() {
            cfg.server_url = v.to_string();
        }
    }
    if let Ok(v) = std::env::var("AGENT_NAME") {
        let v = v.trim();
        if !v.is_empty() {
            cfg.agent_name = v.to_string();
        }
    }
    if let Ok(v) = std::env::var("AGENT_PASSWORD") {
        let v = v.trim();
        if !v.is_empty() {
            cfg.agent_password = v.to_string();
        }
    }

    cfg
}

/// Persist configuration to disk, creating parent directories as needed.
pub fn save_config(config: &Config) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(config)?)?;
    Ok(())
}

// ─── Agent status ─────────────────────────────────────────────────────────────

/// Real-time connection status of the agent, shared between the background
/// tokio thread (writer) and the GUI thread (reader).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AgentStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    /// A human-readable description of the last error.
    Error(String),
}
