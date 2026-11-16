//! Opt-in live surfaces and audio retention carried by recording snapshots.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionMode {
    #[default]
    Overlay,
    MenuBar,
    Both,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveCaptions {
    pub enabled: bool,
    pub mode: CaptionMode,
    pub position: CaptionPosition,
    pub opacity: f64,
    pub font_pt: u32,
    pub max_lines: usize,
    pub width_px: u32,
    pub always_on_top: bool,
    pub shadow: bool,
    pub menu_bar_max_chars: usize,
}
impl Default for LiveCaptions {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: CaptionMode::Overlay,
            position: CaptionPosition::BottomRight,
            opacity: 0.9,
            font_pt: 22,
            max_lines: 3,
            width_px: 600,
            always_on_top: true,
            shadow: true,
            menu_bar_max_chars: 40,
        }
    }
}
impl LiveCaptions {
    pub fn normalize(&mut self) {
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.2, 1.0)
        } else {
            0.9
        };
        self.font_pt = self.font_pt.clamp(12, 64);
        self.max_lines = self.max_lines.clamp(1, 8);
        self.width_px = self.width_px.clamp(280, 1200);
        self.menu_bar_max_chars = self.menu_bar_max_chars.clamp(10, 80);
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawWavAction {
    Delete,
    #[default]
    TranscodeCompressed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetentionPolicy {
    pub enabled: bool,
    pub min_age_days: u32,
    pub raw_wav_action: RawWavAction,
    pub bitrate_kbps: u32,
}
impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            min_age_days: 30,
            raw_wav_action: RawWavAction::TranscodeCompressed,
            bitrate_kbps: 96,
        }
    }
}
impl RetentionPolicy {
    pub fn normalize(&mut self) {
        self.min_age_days = self.min_age_days.clamp(1, 36500);
        self.bitrate_kbps = self.bitrate_kbps.clamp(32, 320);
    }
}

#[cfg(test)]
mod tests {
    use crate::{setups::Snapshot, AppConfig, CONFIG_VERSION};
    #[test]
    fn migration_leaves_live_surfaces_and_retention_disabled() {
        let mut config: AppConfig = serde_json::from_str(r#"{"version":1}"#).unwrap();
        config.migrate();
        assert_eq!(config.version, CONFIG_VERSION);
        assert!(!config.live_captions.enabled);
        assert!(!config.retention.enabled);
        let snapshot: Snapshot = serde_json::from_str("{}").unwrap();
        assert!(!snapshot.retention.enabled);
    }
    #[test]
    fn policies_are_independent_and_round_trip_with_snapshots() {
        let mut work = AppConfig::default();
        work.retention.enabled = true;
        work.retention.min_age_days = 7;
        let saved = serde_json::to_string(&Snapshot::capture(&work)).unwrap();
        let podcast = Snapshot::capture(&AppConfig::default());
        podcast.apply_to(&mut work);
        assert!(!work.retention.enabled);
        let restored: Snapshot = serde_json::from_str(&saved).unwrap();
        restored.apply_to(&mut work);
        assert!(work.retention.enabled);
        assert_eq!(work.retention.min_age_days, 7);
    }
    #[test]
    fn bounds_are_safe_on_import() {
        let mut config: AppConfig = serde_json::from_str(r#"{"live_captions":{"opacity":8,"max_lines":0,"font_pt":999},"retention":{"min_age_days":0,"bitrate_kbps":1}}"#).unwrap();
        config.migrate();
        assert_eq!(config.live_captions.opacity, 1.0);
        assert_eq!(config.live_captions.max_lines, 1);
        assert_eq!(config.live_captions.font_pt, 64);
        assert_eq!(config.retention.min_age_days, 1);
        assert_eq!(config.retention.bitrate_kbps, 32);
        assert!(!config.retention.enabled);
    }
}
