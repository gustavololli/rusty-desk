use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Configuration for the AI provider and assistant preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub assistant_name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub stt_model: String,
    /// When true (or when `api_key` is empty and URL is not local), uses the built-in local mock.
    pub force_mock: bool,
    pub tts_enabled: bool,
    #[serde(default = "default_true")]
    pub sfx_enabled: bool,
    pub organize_target_dir: String,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_desktop = directories::UserDirs::new()
            .and_then(|u| u.desktop_dir().map(|p| p.to_string_lossy().to_string()))
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{home}/Escritorio")
            });

        Self {
            assistant_name: "Rusty".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: "gpt-4o-mini".to_string(),
            stt_model: "whisper-1".to_string(),
            force_mock: false,
            tts_enabled: false,
            sfx_enabled: true,
            organize_target_dir: default_desktop,
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(proj) = ProjectDirs::from("com", "rustydesk", "rusty-desk") {
            proj.config_dir().join("config.json")
        } else {
            PathBuf::from("rusty-desk-config.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Self>(&raw) {
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Returns true if we should use the local mock provider instead of calling an HTTP endpoint.
    pub fn should_use_mock(&self) -> bool {
        if self.force_mock {
            return true;
        }
        let is_local = self.base_url.contains("localhost") || self.base_url.contains("127.0.0.1");
        self.api_key.trim().is_empty() && !is_local
    }
}
