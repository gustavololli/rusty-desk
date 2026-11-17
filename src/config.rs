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

/// Default screen anchor position when switching to Island mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IslandPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
    #[serde(alias = "Current")]
    Custom,
}

impl IslandPosition {
    pub fn label(self) -> &'static str {
        match self {
            Self::Top => "Arriba",
            Self::Bottom => "Abajo",
            Self::Left => "Izquierda",
            Self::Right => "Derecha",
            Self::Custom => "Personalizada",
        }
    }
}

/// Computes the target `[x, y]` outer window position for Island mode in logical coordinates,
/// clamped within `bounds` `(x, y, w, h)`.
/// Returns `None` for `IslandPosition::Custom` when `custom_pos` is `None`.
pub fn compute_island_position(
    anchor: IslandPosition,
    island_size: [f32; 2],
    bounds: Option<(f32, f32, f32, f32)>,
    margin: f32,
    custom_pos: Option<[f32; 2]>,
) -> Option<[f32; 2]> {
    let margin = if margin.is_finite() {
        margin.clamp(0.0, 128.0)
    } else {
        12.0
    };
    let (bx, by, bw, bh) = match bounds {
        Some((x, y, w, h)) if w > island_size[0] && h > island_size[1] => (x, y, w, h),
        _ => (0.0, 0.0, 1920.0, 1080.0),
    };
    let [w, h] = island_size;
    let raw = match anchor {
        IslandPosition::Custom => custom_pos?,
        IslandPosition::Top => [bx + (bw - w) * 0.5, by + margin],
        IslandPosition::Bottom => [bx + (bw - w) * 0.5, by + bh - h - margin],
        IslandPosition::Left => [bx + margin, by + (bh - h) * 0.5],
        IslandPosition::Right => [bx + bw - w - margin, by + (bh - h) * 0.5],
    };
    Some(clamp_window_position(
        raw,
        island_size,
        Some((bx, by, bw, bh)),
    ))
}

/// Clamps an arbitrary window position `[x, y]` with `size` `[w, h]` inside screen `bounds`.
pub fn clamp_window_position(
    pos: [f32; 2],
    size: [f32; 2],
    bounds: Option<(f32, f32, f32, f32)>,
) -> [f32; 2] {
    if !pos[0].is_finite() || !pos[1].is_finite() {
        return [60.0, 60.0];
    }
    let (min_x, min_y, max_x, max_y) = match bounds {
        Some((bx, by, bw, bh)) if bw > size[0] && bh > size[1] => {
            (bx, by, bx + bw - size[0], by + bh - size[1])
        }
        _ => (0.0, 0.0, 1920.0 - size[0], 1080.0 - size[1]),
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
    #[serde(default)]
    pub island_position: IslandPosition,
    #[serde(default)]
    pub island_custom_position: Option<[f32; 2]>,
    #[serde(default = "default_island_margin")]
    pub island_margin: f32,
    #[serde(default)]
    pub island_auto_hide: bool,
    #[serde(default = "default_island_hide_delay")]
    pub island_hide_delay: u32,
    #[serde(default = "default_true")]
    pub island_show_on_hover: bool,
    #[serde(default = "default_true")]
    pub island_keep_visible_during_playback: bool,
    #[serde(default = "default_true")]
    pub island_keep_visible_during_ai: bool,
    #[serde(default = "default_audio_volume")]
    pub audio_volume: u8,
    #[serde(default)]
    pub audio_muted: bool,
}

fn default_true() -> bool {
    true
}

fn default_island_margin() -> f32 {
    12.0
}

fn default_island_hide_delay() -> u32 {
    5
}

fn default_audio_volume() -> u8 {
    80
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
            island_position: IslandPosition::default(),
            island_custom_position: None,
            island_margin: default_island_margin(),
            island_auto_hide: false,
            island_hide_delay: default_island_hide_delay(),
            island_show_on_hover: true,
            island_keep_visible_during_playback: true,
            island_keep_visible_during_ai: true,
            audio_volume: default_audio_volume(),
            audio_muted: false,
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
        assert_eq!(cfg.island_position, IslandPosition::Top);
        assert_eq!(cfg.island_hide_delay, 5);
        assert!(cfg.island_show_on_hover);
        assert!(cfg.island_keep_visible_during_playback);
        assert_eq!(cfg.audio_volume, 80);
        assert!(!cfg.audio_muted);
    }

    #[test]
    fn compute_island_position_anchors_to_edges() {
        let bounds = Some((0.0, 0.0, 1920.0, 1080.0));
        let size = [360.0, 66.0];
        assert_eq!(
            compute_island_position(IslandPosition::Custom, size, bounds, 16.0, None),
            None
        );
        assert_eq!(
            compute_island_position(
                IslandPosition::Custom,
                size,
                bounds,
                16.0,
                Some([-50.0, 2000.0])
            ),
            Some([0.0, 1014.0])
        );
        assert_eq!(
            compute_island_position(IslandPosition::Top, size, bounds, 16.0, None),
            Some([780.0, 16.0])
        );
        assert_eq!(
            compute_island_position(IslandPosition::Bottom, size, bounds, 16.0, None),
            Some([780.0, 998.0])
        );
        assert_eq!(
            compute_island_position(IslandPosition::Left, size, bounds, 16.0, None),
            Some([16.0, 507.0])
        );
        assert_eq!(
            compute_island_position(IslandPosition::Right, size, bounds, 16.0, None),
            Some([1544.0, 507.0])
        );
    }

    #[test]
    fn clamp_window_position_keeps_expanded_on_screen() {
        let bounds = Some((0.0, 0.0, 1920.0, 1080.0));
        assert_eq!(
            clamp_window_position([1544.0, 998.0], [450.0, 650.0], bounds),
            [1470.0, 430.0]
        );
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
