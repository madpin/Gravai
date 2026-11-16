//! Startup audio source health check.
//!
//! After a recording starts, each enabled source gets `audio.source_check_secs`
//! to deliver audible sound. This module holds the decision logic as plain
//! data in, warnings out; the app feeds it volume levels once a second and
//! shows what it returns.

use crate::AudioConfig;
use serde::Serialize;
use std::collections::HashSet;

/// Event source names, as used by `VolumeLevel` events.
pub const MICROPHONE: &str = "microphone";
pub const SYSTEM_AUDIO: &str = "system_audio";

/// Levels above this count as sound. Higher than room noise (about -60 dB),
/// far below speech (about -20 dB) and above the -91 dB digital silence
/// ScreenCaptureKit delivers when the captured app is quiet.
pub const AUDIBLE_DB: f64 = -50.0;

/// What the warning is about.
pub mod kind {
    /// The selected app is not running; all system audio is captured instead.
    pub const APP_NOT_RUNNING: &str = "app_not_running";
    /// The source delivered no audio data at all.
    pub const NO_AUDIO: &str = "no_audio";
    /// The source delivered audio, but only silence.
    pub const SILENT: &str = "silent";
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceWarning {
    pub source: &'static str,
    pub kind: &'static str,
    pub message: String,
}

/// Warning for a selected app that is not among the running apps.
pub fn app_not_running_warning(bundle_id: &str) -> SourceWarning {
    SourceWarning {
        source: SYSTEM_AUDIO,
        kind: kind::APP_NOT_RUNNING,
        message: format!(
            "Selected app {bundle_id} is not running, capturing all system audio instead."
        ),
    }
}

/// Loudest level seen per source. `None` means no level arrived at all.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Peaks {
    microphone: Option<f64>,
    system_audio: Option<f64>,
}

/// What changed on one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceHealthUpdate {
    /// Warnings to show now. Each source appears once until it clears.
    pub raised: Vec<SourceWarning>,
    /// Sources whose warning no longer applies.
    pub cleared: Vec<&'static str>,
}

/// Tracks the health of the sources for one recording.
#[derive(Debug, Default)]
pub struct SourceHealth {
    peaks: Peaks,
    elapsed_secs: u64,
    target_app_missing: bool,
    active: HashSet<&'static str>,
}

impl SourceHealth {
    /// Forget everything; call when a recording starts or stops. Returns the
    /// sources that had a warning showing, which no longer apply.
    pub fn reset(&mut self) -> Vec<&'static str> {
        let cleared = self.active.iter().copied().collect();
        *self = Self::default();
        cleared
    }

    /// Note a level reading from a source. Unknown sources are ignored.
    pub fn record_level(&mut self, source: &str, db: f64) {
        let peak = match source {
            MICROPHONE => &mut self.peaks.microphone,
            SYSTEM_AUDIO => &mut self.peaks.system_audio,
            _ => return,
        };
        *peak = Some(peak.map_or(db, |p| p.max(db)));
    }

    /// The selected app was not running when capture started.
    pub fn set_target_app_missing(&mut self) {
        self.target_app_missing = true;
    }

    /// Advance one second of recording and report what changed.
    pub fn tick(&mut self, audio: &AudioConfig) -> SourceHealthUpdate {
        self.elapsed_secs += 1;
        let current = check_sources(
            audio,
            &self.peaks,
            self.elapsed_secs,
            self.target_app_missing,
        );
        let raised = current
            .iter()
            .filter(|w| self.active.insert(w.source))
            .cloned()
            .collect();
        let cleared: Vec<&'static str> = self
            .active
            .iter()
            .copied()
            .filter(|source| current.iter().all(|w| w.source != *source))
            .collect();
        for source in &cleared {
            self.active.remove(source);
        }
        SourceHealthUpdate { raised, cleared }
    }
}

/// Warnings that apply `elapsed_secs` into a recording. Empty while the
/// check window is still open, or when the check is turned off.
fn check_sources(
    audio: &AudioConfig,
    peaks: &Peaks,
    elapsed_secs: u64,
    target_app_missing: bool,
) -> Vec<SourceWarning> {
    if audio.source_check_secs == 0 || elapsed_secs < u64::from(audio.source_check_secs) {
        return Vec::new();
    }
    let mut warnings = Vec::new();
    if audio.microphone.enabled {
        if let Some(reason) = silence_kind(peaks.microphone) {
            let message = match reason {
                kind::NO_AUDIO => {
                    "No audio from the microphone. Check that the right input \
                                   device is selected and connected."
                }
                _ => {
                    "The microphone is silent. Check that it is not muted and that the right \
                      input device is selected."
                }
            };
            warnings.push(SourceWarning {
                source: MICROPHONE,
                kind: reason,
                message: message.into(),
            });
        }
    }
    if audio.system_audio.enabled {
        if let Some(reason) = silence_kind(peaks.system_audio) {
            let bundle_id = audio.system_audio.app_bundle_id.as_str();
            let message = if bundle_id.is_empty() || target_app_missing {
                "No system audio. Nothing may be playing, or Screen Recording permission \
                 is missing for Gravai."
                    .to_string()
            } else {
                format!(
                    "No audio from {bundle_id}. It may not be playing sound, is muted, or \
                     Screen Recording permission is missing. Choose All system audio in \
                     Settings to capture everything."
                )
            };
            warnings.push(SourceWarning {
                source: SYSTEM_AUDIO,
                kind: reason,
                message,
            });
        }
    }
    warnings
}

fn silence_kind(peak: Option<f64>) -> Option<&'static str> {
    match peak {
        None => Some(kind::NO_AUDIO),
        Some(db) if db > AUDIBLE_DB => None,
        Some(_) => Some(kind::SILENT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(mic: bool, sys: bool, bundle: &str, secs: u32) -> AudioConfig {
        let mut audio = AudioConfig::default();
        audio.microphone.enabled = mic;
        audio.system_audio.enabled = sys;
        audio.system_audio.app_bundle_id = bundle.into();
        audio.source_check_secs = secs;
        audio
    }

    fn tick_n(health: &mut SourceHealth, audio: &AudioConfig, n: u32) -> SourceHealthUpdate {
        let mut last = SourceHealthUpdate::default();
        for _ in 0..n {
            last = health.tick(audio);
        }
        last
    }

    #[test]
    fn nothing_is_reported_before_the_window_closes() {
        let audio = audio(true, true, "", 8);
        let mut health = SourceHealth::default();
        let update = tick_n(&mut health, &audio, 7);
        assert_eq!(update, SourceHealthUpdate::default());
        let update = health.tick(&audio);
        assert_eq!(update.raised.len(), 2);
    }

    #[test]
    fn zero_disables_the_check() {
        let audio = audio(true, true, "", 0);
        let mut health = SourceHealth::default();
        assert_eq!(
            tick_n(&mut health, &audio, 60),
            SourceHealthUpdate::default()
        );
    }

    #[test]
    fn disabled_sources_are_not_checked() {
        let audio = audio(false, true, "", 3);
        let mut health = SourceHealth::default();
        let update = tick_n(&mut health, &audio, 3);
        assert_eq!(update.raised.len(), 1);
        assert_eq!(update.raised[0].source, SYSTEM_AUDIO);
    }

    #[test]
    fn audible_source_passes_and_digital_silence_does_not() {
        let audio = audio(true, true, "", 3);
        let mut health = SourceHealth::default();
        health.record_level(MICROPHONE, -20.0);
        health.record_level(SYSTEM_AUDIO, -91.0);
        health.record_level(SYSTEM_AUDIO, -60.0);
        let update = tick_n(&mut health, &audio, 3);
        assert_eq!(update.raised.len(), 1);
        assert_eq!(update.raised[0].source, SYSTEM_AUDIO);
        assert_eq!(update.raised[0].kind, kind::SILENT);
    }

    #[test]
    fn threshold_is_exclusive() {
        let audio = audio(true, false, "", 1);
        let mut health = SourceHealth::default();
        health.record_level(MICROPHONE, AUDIBLE_DB);
        assert_eq!(health.tick(&audio).raised.len(), 1);
        health.record_level(MICROPHONE, AUDIBLE_DB + 0.1);
        assert_eq!(health.tick(&audio).cleared, vec![MICROPHONE]);
    }

    #[test]
    fn no_levels_at_all_is_distinct_from_silence() {
        let audio = audio(true, false, "", 2);
        let mut health = SourceHealth::default();
        let update = tick_n(&mut health, &audio, 2);
        assert_eq!(update.raised[0].kind, kind::NO_AUDIO);
        assert!(update.raised[0]
            .message
            .contains("No audio from the microphone"));
    }

    #[test]
    fn alert_is_raised_once_and_cleared_when_sound_arrives() {
        let audio = audio(true, false, "", 2);
        let mut health = SourceHealth::default();
        health.record_level(MICROPHONE, -91.0);
        assert_eq!(tick_n(&mut health, &audio, 2).raised.len(), 1);
        // Still silent: no repeat.
        assert_eq!(
            tick_n(&mut health, &audio, 5),
            SourceHealthUpdate::default()
        );
        // Sound arrives: cleared once.
        health.record_level(MICROPHONE, -30.0);
        let update = health.tick(&audio);
        assert_eq!(update.cleared, vec![MICROPHONE]);
        assert!(update.raised.is_empty());
        assert_eq!(health.tick(&audio), SourceHealthUpdate::default());
    }

    #[test]
    fn targeted_app_is_named_in_the_system_warning() {
        let audio = audio(false, true, "us.zoom.xos", 1);
        let mut health = SourceHealth::default();
        let update = health.tick(&audio);
        assert!(update.raised[0].message.contains("us.zoom.xos"));
        assert!(update.raised[0].message.contains("Screen Recording"));
    }

    #[test]
    fn missing_app_falls_back_to_the_all_system_audio_wording() {
        let audio = audio(false, true, "us.zoom.xos", 1);
        let mut health = SourceHealth::default();
        health.set_target_app_missing();
        let update = health.tick(&audio);
        assert!(!update.raised[0].message.contains("us.zoom.xos"));
        assert!(update.raised[0].message.starts_with("No system audio"));
        assert!(app_not_running_warning("us.zoom.xos")
            .message
            .contains("capturing all system audio instead"));
    }

    #[test]
    fn reset_starts_a_fresh_window() {
        let audio = audio(true, false, "", 2);
        let mut health = SourceHealth::default();
        health.record_level(MICROPHONE, -10.0);
        health.set_target_app_missing();
        assert!(health.reset().is_empty());
        assert_eq!(health.tick(&audio), SourceHealthUpdate::default());
        assert_eq!(health.tick(&audio).raised.len(), 1);
        // A warning that is showing is handed back so it can be dismissed.
        assert_eq!(health.reset(), vec![MICROPHONE]);
    }

    #[test]
    fn unknown_sources_are_ignored() {
        let audio = audio(true, false, "", 1);
        let mut health = SourceHealth::default();
        health.record_level("file", -10.0);
        assert_eq!(health.tick(&audio).raised.len(), 1);
    }
}
