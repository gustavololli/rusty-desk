use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Size preset for the floating desktop pet mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PetSize {
    Small,
    #[default]
    Normal,
    Large,
}

impl PetSize {
    pub fn pixels(self) -> f32 {
        match self {
            Self::Small => 64.0,
            Self::Normal => 80.0,
            Self::Large => 104.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "Pequeño (64px)",
            Self::Normal => "Normal (80px)",
            Self::Large => "Grande (104px)",
        }
    }
}

/// Expressiveness level for procedural pet animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PetIntensity {
    Subtle,
    #[default]
    Normal,
    Expressive,
}

impl PetIntensity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Subtle => "Sutil",
            Self::Normal => "Normal",
            Self::Expressive => "Expresiva",
        }
    }
}

/// Configuration for desktop pet mode and procedural animations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub always_on_top: bool,
    #[serde(default = "default_true")]
    pub follow_cursor: bool,
    #[serde(default = "default_true")]
    pub auto_animations: bool,
    #[serde(default = "default_true")]
    pub celebrate_tasks: bool,
    #[serde(default = "default_sleep_mins")]
    pub sleep_after_mins: u32,
    #[serde(default)]
    pub size: PetSize,
    #[serde(default)]
    pub intensity: PetIntensity,
    #[serde(default)]
    pub reduce_animations: bool,
    #[serde(default)]
    pub pet_pos: Option<[f32; 2]>,
}

fn default_sleep_mins() -> u32 {
    5
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            always_on_top: true,
            follow_cursor: true,
            auto_animations: true,
            celebrate_tasks: true,
            sleep_after_mins: 5,
            size: PetSize::Normal,
            intensity: PetIntensity::Normal,
            reduce_animations: false,
            pet_pos: None,
        }
    }
}

/// Clamps a saved pet window position so it stays visible within screen bounds.
/// Falls back to a conservative visible range when screen bounds are unknown.
pub fn clamp_pet_position(
    pos: [f32; 2],
    pet_px: f32,
    bounds: Option<(f32, f32, f32, f32)>,
) -> [f32; 2] {
    if !pos[0].is_finite() || !pos[1].is_finite() {
        return [60.0, 60.0];
    }
    let (min_x, min_y, max_x, max_y) = match bounds {
        Some((bx, by, bw, bh)) if bw > pet_px && bh > pet_px => {
            (bx, by, bx + bw - pet_px, by + bh - pet_px)
        }
        _ => (0.0, 0.0, 1920.0 - pet_px, 1080.0 - pet_px),
    };
    [pos[0].clamp(min_x, max_x), pos[1].clamp(min_y, max_y)]
}

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
    #[serde(default)]
    pub pet: PetConfig,
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
            pet: PetConfig::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_json_without_pet_loads_defaults() {
        let legacy_json = r#"{
            "assistant_name": "Rusty",
            "base_url": "https://api.openai.com/v1",
            "api_key": "",
            "model": "gpt-4o-mini",
            "stt_model": "whisper-1",
            "force_mock": false,
            "tts_enabled": false,
            "sfx_enabled": true,
            "organize_target_dir": "/tmp"
        }"#;
        let cfg: AppConfig = serde_json::from_str(legacy_json).expect("legacy json should parse");
        assert!(cfg.pet.enabled);
        assert!(cfg.pet.always_on_top);
        assert!(cfg.pet.follow_cursor);
        assert_eq!(cfg.pet.size, PetSize::Normal);
        assert_eq!(cfg.pet.sleep_after_mins, 5);
        assert_eq!(cfg.pet.pet_pos, None);
    }

    #[test]
    fn clamp_pet_position_keeps_window_on_screen() {
        let clamped = clamp_pet_position([-200.0, 5000.0], 80.0, Some((0.0, 0.0, 1920.0, 1080.0)));
        assert_eq!(clamped, [0.0, 1000.0]);

        let nan_clamped =
            clamp_pet_position([f32::NAN, 100.0], 80.0, Some((0.0, 0.0, 1920.0, 1080.0)));
        assert_eq!(nan_clamped, [60.0, 60.0]);
    }
}
