//! Persistent configuration and shared runtime state for the agent.
//!
//! ## On-disk format (`config.dat`)
//!
//! The file is **encrypted with Windows DPAPI** ([`CryptProtectData`]), not merely
//! base64-encoded. Ciphertext is bound to the **current Windows user and this PC**;
//! another user or another machine cannot decrypt it from the file alone. This matches
//! how many desktop apps protect local secrets (browser profiles, Credential Manager
//! exports, some enterprise agents).
//!
//! Legacy: older builds stored base64(JSON) or plain `config.json`; those are still read
//! and are rewritten in the new format on next save.
//!
//! [`CryptProtectData`]: https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata

use base64::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tracing::warn;
use windows_dpapi::{decrypt_data, encrypt_data, Scope};

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

/// Optional app-specific entropy so unrelated DPAPI blobs are never mistaken for ours.
const CONFIG_DPAPI_ENTROPY: &[u8] = b"sentinel-agent-config\0";

/// Path to the encrypted config file.
///
/// On Windows: `%LOCALAPPDATA%\sentinel\config.dat`
pub fn config_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sentinel")
        .join("config.dat")
}

fn parse_config_json(s: &str) -> Option<Config> {
    serde_json::from_str::<Config>(s).ok()
}

/// Try legacy base64-wrapped JSON (`config.dat` from older builds).
fn try_load_legacy_base64_dat(bytes: &[u8]) -> Option<Config> {
    let s = std::str::from_utf8(bytes).ok()?.trim();
    let dec = BASE64_STANDARD.decode(s).ok()?;
    let s = String::from_utf8(dec).ok()?;
    parse_config_json(&s)
}

/// Try DPAPI-encrypted JSON (current format).
fn try_load_dpapi_dat(bytes: &[u8]) -> Option<Config> {
    let dec = decrypt_data(bytes, Scope::User, Some(CONFIG_DPAPI_ENTROPY)).ok()?;
    let s = String::from_utf8(dec).ok()?;
    parse_config_json(&s)
}

/// Load configuration from disk; falls back to `Config::default()` on any error.
pub fn load_config() -> Config {
    let path = config_path();
    let old_path = path.with_extension("json");

    let mut cfg = Config::default();

    // 1. `config.dat` — prefer DPAPI, then legacy base64(JSON)
    if let Ok(bytes) = std::fs::read(&path) {
        if !bytes.is_empty() {
            if let Some(c) = try_load_dpapi_dat(&bytes) {
                cfg = c;
            } else if let Some(c) = try_load_legacy_base64_dat(&bytes) {
                warn!(
                    "Loaded legacy base64 config.dat; it will be upgraded to DPAPI on next save"
                );
                cfg = c;
            }
        }
    } else if let Ok(json) = std::fs::read_to_string(&old_path) {
        // 2. Very old plain `config.json`
        if let Ok(c) = serde_json::from_str::<Config>(&json) {
            warn!("Loaded legacy config.json; it will be replaced by encrypted config.dat on next save");
            cfg = c;
        }
    }

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
///
/// Writes **DPAPI-encrypted** bytes (not base64 text). Requires the same Windows user
/// and machine to decrypt.
pub fn save_config(config: &Config) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string(config)?;
    let encrypted = encrypt_data(json.as_bytes(), Scope::User, Some(CONFIG_DPAPI_ENTROPY))?;
    std::fs::write(&path, encrypted)?;

    // Attempt to clean up the old readable json file safely
    let old_path = path.with_extension("json");
    let _ = std::fs::remove_file(old_path);

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
