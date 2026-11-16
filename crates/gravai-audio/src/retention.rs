//! Scheduled audio-only retention. Policy is frozen in each session snapshot.
//! Existing recordings without a policy are never enrolled retroactively.
use crate::encoder::{self, ExportFormat};
use chrono::{DateTime, Utc};
use gravai_config::{
    live_accessibility::{RawWavAction, RetentionPolicy},
    setups::Snapshot,
};
use gravai_storage::{AudioArchivalRecord, Database};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize)]
pub struct RetentionReport {
    pub processed: usize,
    pub freed_bytes: u64,
    pub errors: Vec<String>,
}

/// Manifest is the atomic commit point for a full-timeline compressed source.
/// Included capture directories must not be mixed a second time on continuation.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ArchiveManifest {
    pub file: String,
    pub included: Vec<String>,
}
pub(crate) fn manifest(dir: &Path) -> Result<Option<ArchiveManifest>, String> {
    let path = dir.join("audio_archive.json");
    if !path.exists() {
        return Ok(None);
    }
    let data: ArchiveManifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if !data.file.starts_with("archive_")
        || !data.file.ends_with(".m4a")
        || Path::new(&data.file).components().count() != 1
        || !dir.join(&data.file).is_file()
    {
        return Err("Invalid or missing compressed archive; sources kept".into());
    }
    Ok(Some(data))
}

pub fn eligible(
    policy: &RetentionPolicy,
    state: &str,
    ended_at: Option<&str>,
    now: DateTime<Utc>,
) -> bool {
    policy.enabled
        && state == "stopped"
        && ended_at
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|ended| {
                now.signed_duration_since(ended).num_seconds()
                    >= i64::from(policy.min_age_days.max(1)) * 86400
            })
}

pub fn run(
    db: &Database,
    sessions_dir: &Path,
    now: DateTime<Utc>,
) -> Result<RetentionReport, String> {
    let mut report = RetentionReport::default();
    for session in db.list_sessions().map_err(|e| e.to_string())? {
        let Some(json) = db
            .get_session_snapshot(&session.id)
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let Ok(snapshot) = serde_json::from_str::<Snapshot>(&json) else {
            continue;
        };
        if !eligible(
            &snapshot.retention,
            &session.state,
            session.ended_at.as_deref(),
            now,
        ) {
            continue;
        }
        // Old custom output folders may be shared across sessions. Never prune them.
        if snapshot
            .recording
            .output_folder
            .as_ref()
            .is_some_and(|p| !p.is_empty())
        {
            report.errors.push(format!(
                "{}: retention skipped shared custom audio folder",
                session.id
            ));
            continue;
        }
        if Path::new(&session.id).components().count() != 1 || session.id.starts_with('.') {
            continue;
        }
        let dir = sessions_dir.join(&session.id);
        if !dir.is_dir() {
            continue;
        }
        let lock = encoder::session_lock(&dir);
        let _guard = lock.lock().map_err(|_| "Audio lock poisoned")?;
        match apply(db, &session.id, &dir, &snapshot.retention, now) {
            Ok(Some(freed)) => {
                report.processed += 1;
                report.freed_bytes += freed;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::error!("Retention {}: {error}", session.id);
                report.errors.push(format!("{}: {error}", session.id));
                let mut record = db
                    .audio_archival(&session.id)
                    .map_err(|e| e.to_string())?
                    .unwrap_or(AudioArchivalRecord {
                        session_id: session.id,
                        replay_path: None,
                        mime: None,
                        state: "failed".into(),
                        raw_deleted_at: None,
                        error: None,
                    });
                record.error = Some(error);
                db.set_audio_archival(&record).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(report)
}
/// Resolve replay without constructing a partial timeline from a delete-only
/// session whose canonical audio is no longer available.
pub fn resolve_replay(db: &Database, id: &str, dir: &Path, force: bool) -> Result<PathBuf, String> {
    if db
        .audio_archival(id)
        .map_err(|e| e.to_string())?
        .is_some_and(|a| a.state == "audio_unavailable")
    {
        return Err("Audio no longer available; transcript retained".into());
    }
    encoder::resolve_playback_path(dir, force)
}

/// Reconcile a user-requested removal of all audio with the archive manifest.
pub fn forget_audio(db: &Database, id: &str, dir: &Path) -> Result<(), String> {
    let path = dir.join("audio_archive.json");
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    db.set_audio_archival(&AudioArchivalRecord {
        session_id: id.into(),
        replay_path: None,
        mime: None,
        state: "audio_unavailable".into(),
        raw_deleted_at: Some(Utc::now().to_rfc3339()),
        error: None,
    })
    .map_err(|e| e.to_string())
}

fn apply(
    db: &Database,
    id: &str,
    dir: &Path,
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> Result<Option<u64>, String> {
    let dirs = capture_dirs(dir)?;
    let mut raw = Vec::new();
    for (_, capture) in &dirs {
        raw.extend(encoder::retention_wavs(capture)?);
    }
    if raw.is_empty() {
        // Recover a crash after the last unlink but before the final DB stamp.
        if let Some(mut record) = db.audio_archival(id).map_err(|e| e.to_string())? {
            if record.raw_deleted_at.is_none()
                && matches!(record.state.as_str(), "compressed" | "audio_unavailable")
            {
                record.raw_deleted_at = Some(now.to_rfc3339());
                record.error = None;
                db.set_audio_archival(&record).map_err(|e| e.to_string())?;
            }
        }
        return Ok(None);
    }
    let replay = if policy.raw_wav_action == RawWavAction::TranscodeCompressed {
        Some(archive(dir, &dirs, policy.bitrate_kbps.clamp(32, 320))?)
    } else {
        manifest(dir)?.map(|m| dir.join(m.file)).or_else(|| {
            let path = dir.join(if dir.join("continuations").exists() {
                "_timeline.m4a"
            } else {
                "playback.m4a"
            });
            encoder::timeline_cache_fresh(&path, dir).then_some(path)
        })
    };
    // Persist the published replay path BEFORE deleting any sources. A retry
    // reconciles partial deletion; transcript/summary tables are never touched.
    let mut record = AudioArchivalRecord {
        session_id: id.into(),
        replay_path: replay.as_ref().map(|p| p.display().to_string()),
        mime: replay.as_ref().map(|_| "audio/mp4".into()),
        state: if replay.is_some() {
            "compressed"
        } else {
            "audio_unavailable"
        }
        .into(),
        raw_deleted_at: None,
        error: None,
    };
    db.set_audio_archival(&record).map_err(|e| e.to_string())?;
    let _guard = encoder::compaction_guard()?;
    let mut freed = 0;
    for path in raw {
        let bytes = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
        freed += bytes;
    }
    record.raw_deleted_at = Some(now.to_rfc3339());
    db.set_audio_archival(&record).map_err(|e| e.to_string())?;
    Ok(Some(freed))
}
pub(crate) fn capture_dirs(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut dirs = vec![(String::new(), dir.to_path_buf())];
    let root = dir.join("continuations");
    if root.exists() {
        for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                dirs.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
        }
    }
    dirs.sort();
    Ok(dirs)
}
fn archive(dir: &Path, dirs: &[(String, PathBuf)], bitrate: u32) -> Result<PathBuf, String> {
    let old = manifest(dir)?;
    let included: Vec<_> = dirs.iter().map(|(name, _)| name.clone()).collect();
    if let Some(m) = &old {
        if m.included == included && encoder::verified_duration(&dir.join(&m.file))? > 0.0 {
            return Ok(dir.join(&m.file));
        }
    }
    let scratch = dir.join("_playback_retention.wav");
    let nonce = Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let file = format!("archive_{nonce}.m4a");
    let output = dir.join(&file);
    let result = (|| {
        encoder::export_session_timeline(dir, &scratch, ExportFormat::Wav, bitrate)?;
        let reader = hound::WavReader::open(&scratch).map_err(|e| e.to_string())?;
        let seconds = reader.duration() as f64 / reader.spec().sample_rate as f64;
        drop(reader);
        encoder::export_audio(&scratch, &output, ExportFormat::M4aAac, bitrate)?;
        let duration = encoder::verified_duration(&output)?;
        let compressed = std::fs::metadata(&output).map_err(|e| e.to_string())?.len();
        let pcm_bytes = std::fs::metadata(&scratch)
            .map_err(|e| e.to_string())?
            .len();
        if seconds <= 0.0
            || (seconds - duration).abs() > (seconds * 0.01).max(0.25)
            || compressed <= 100
            || compressed >= pcm_bytes
        {
            return Err("Compressed archive verification failed; raw WAVs kept".into());
        }
        let manifest = ArchiveManifest { file, included };
        let temp = dir.join("audio_archive.building.json");
        std::fs::write(
            &temp,
            serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(temp, dir.join("audio_archive.json")).map_err(|e| e.to_string())?;
        Ok(output.clone())
    })();
    let _ = std::fs::remove_file(scratch);
    if result.is_err() {
        let _ = std::fs::remove_file(&output);
    }
    if result.is_ok() {
        if let Some(old) = old {
            let _ = std::fs::remove_file(dir.join(old.file));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use gravai_storage::{SessionRecord, UtteranceRecord};
    struct Fixture {
        root: PathBuf,
        db: Database,
        now: DateTime<Utc>,
    }
    impl Fixture {
        fn new(action: RawWavAction) -> Self {
            let now = Utc::now();
            let root = std::env::temp_dir().join(format!(
                "gravai-retention-{}",
                now.timestamp_nanos_opt().unwrap()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let db = Database::open_in_memory().unwrap();
            let mut policy = RetentionPolicy {
                enabled: true,
                min_age_days: 7,
                raw_wav_action: action,
                ..Default::default()
            };
            for (id, age, state) in [
                ("old", 8, "stopped"),
                ("young", 6, "stopped"),
                ("active", 8, "recording"),
                ("disabled", 8, "stopped"),
            ] {
                db.create_session(&SessionRecord {
                    id: id.into(),
                    started_at: (now - chrono::Duration::days(age)).to_rfc3339(),
                    ended_at: Some((now - chrono::Duration::days(age)).to_rfc3339()),
                    duration_seconds: Some(2.0),
                    title: None,
                    meeting_app: None,
                    state: state.into(),
                })
                .unwrap();
                policy.enabled = id != "disabled";
                let snapshot = Snapshot {
                    retention: policy.clone(),
                    ..Default::default()
                };
                db.set_session_snapshot(id, &serde_json::to_string(&snapshot).unwrap())
                    .unwrap();
                let dir = root.join(id);
                std::fs::create_dir(&dir).unwrap();
                tone(&dir.join("mic.wav"));
            }
            db.insert_bookmark("old", 300, Some("keep")).unwrap();
            db.upsert_session_summary("old", Some("keep summary"), None, None, None, None)
                .unwrap();
            let utterance_id = db
                .insert_utterance(&UtteranceRecord {
                    id: 0,
                    session_id: "old".into(),
                    timestamp: now.to_rfc3339(),
                    source: "mic".into(),
                    speaker: None,
                    text: "keep transcript".into(),
                    confidence: None,
                    start_ms: Some(0),
                    end_ms: Some(1000),
                    sentiment_label: None,
                    sentiment_score: None,
                    emotions_json: None,
                    corrected_text: Some("Keep transcript".into()),
                    correction_status: None,
                    correction_provider: None,
                    corrected_at: None,
                })
                .unwrap();
            db.update_utterance_correction(utterance_id, "Keep transcript", "test", "corrected")
                .unwrap();
            Self { root, db, now }
        }
        fn durable_content(&self) {
            assert_eq!(self.db.list_bookmarks("old").unwrap().len(), 1);
            assert!(self.db.get_session_summary("old").unwrap().is_some());
            assert_eq!(
                self.db.get_utterances("old").unwrap()[0]
                    .corrected_text
                    .as_deref(),
                Some("Keep transcript")
            );
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn tone(path: &Path) {
        let mut wav = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for i in 0..32000 {
            wav.write_sample(((i as f32 * 0.08).sin() * 10000.0) as i16)
                .unwrap();
        }
        wav.finalize().unwrap();
    }
    #[test]
    fn age_boundary_uses_end_time_and_requires_explicit_opt_in() {
        let now = Utc::now();
        let mut p = RetentionPolicy {
            enabled: true,
            min_age_days: 7,
            ..Default::default()
        };
        let boundary = (now - chrono::Duration::days(7)).to_rfc3339();
        assert!(eligible(&p, "stopped", Some(&boundary), now));
        assert!(!eligible(&p, "recording", Some(&boundary), now));
        assert!(!eligible(&p, "paused", Some(&boundary), now));
        assert!(!eligible(&p, "stopped", None, now));
        assert!(!eligible(&p, "stopped", Some("invalid"), now));
        assert!(!eligible(
            &p,
            "stopped",
            Some(&(now + chrono::Duration::days(1)).to_rfc3339()),
            now
        ));
        p.enabled = false;
        assert!(!eligible(&p, "stopped", Some(&boundary), now));
    }
    #[test]
    fn delete_only_is_scoped_idempotent_and_keeps_transcript_summary_exports() {
        let f = Fixture::new(RawWavAction::Delete);
        let export = f.root.join("old/export.wav");
        tone(&export);
        let report = run(&f.db, &f.root, f.now).unwrap();
        assert_eq!(report.processed, 1);
        assert!(report.errors.is_empty());
        assert!(!f.root.join("old/mic.wav").exists());
        assert!(export.exists());
        for id in ["young", "active", "disabled"] {
            assert!(f.root.join(id).join("mic.wav").exists());
        }
        assert_eq!(
            f.db.audio_archival("old").unwrap().unwrap().state,
            "audio_unavailable"
        );
        assert_eq!(run(&f.db, &f.root, f.now).unwrap().processed, 0);
        let mut interrupted = f.db.audio_archival("old").unwrap().unwrap();
        interrupted.raw_deleted_at = None;
        f.db.set_audio_archival(&interrupted).unwrap();
        assert_eq!(run(&f.db, &f.root, f.now).unwrap().processed, 0);
        assert!(f
            .db
            .audio_archival("old")
            .unwrap()
            .unwrap()
            .raw_deleted_at
            .is_some());
        f.durable_content();
    }
    #[test]
    fn invalid_source_never_gets_deleted_on_transcode_failure() {
        let f = Fixture::new(RawWavAction::TranscodeCompressed);
        let source = f.root.join("old/mic.wav");
        std::fs::write(&source, b"broken WAV").unwrap();
        let report = run(&f.db, &f.root, f.now).unwrap();
        assert_eq!(report.processed, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(source.exists());
        assert!(f.db.audio_archival("old").unwrap().unwrap().error.is_some());
        f.durable_content();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn transcode_replays_full_timeline_and_continues_after_archival() {
        let f = Fixture::new(RawWavAction::TranscodeCompressed);
        let dir = f.root.join("old");
        let leg = encoder::continuation_directory(&dir, 2000).unwrap();
        tone(&leg.join("system.wav"));
        let report = run(&f.db, &f.root, f.now).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.processed, 1);
        assert!(!dir.join("mic.wav").exists());
        assert!(!leg.join("system.wav").exists());
        let replay = encoder::resolve_playback_path(&dir, false).unwrap();
        assert!((encoder::verified_duration(&replay).unwrap() - 4.0).abs() < 0.25);
        assert!(std::fs::metadata(&replay).unwrap().len() < 128000);
        assert_eq!(run(&f.db, &f.root, f.now).unwrap().processed, 0);
        // A later capture must append to the full saved mix, without replaying
        // previously archived legs a second time or losing the compressed base.
        let next = encoder::continuation_directory(&dir, 4000).unwrap();
        tone(&next.join("mic.wav"));
        let output = dir.join("export.wav");
        encoder::export_session_timeline(&dir, &output, ExportFormat::Wav, 96).unwrap();
        let reader = hound::WavReader::open(output).unwrap();
        assert!((reader.duration() as f64 / reader.spec().sample_rate as f64 - 6.0).abs() < 0.25);
        f.durable_content();
    }
}
