//! Recordings that did not finish cleanly: find them at launch and let the
//! user convert, keep, or delete each one.
//!
//! File work (header repair, folder survey) lives in `gravai_audio::recovery`.
//! This module joins it with what the database knows about each session.

use gravai_audio::recovery::{self, SegmentReport};
use gravai_config::setups::Snapshot;
use gravai_core::AppState;
use gravai_storage::{Database, SessionRecord};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;
use tracing::{info, warn};

/// Where a session's audio lives. A custom output folder from the session's
/// own settings wins over the default `sessions/<id>`.
fn audio_dir(db: &Database, id: &str) -> PathBuf {
    db.get_session_snapshot(id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<Snapshot>(&json).ok())
        .and_then(|s| s.recording.output_folder)
        .map(PathBuf::from)
        .unwrap_or_else(|| gravai_config::sessions_dir().join(id))
}

fn snapshot(db: &Database, id: &str) -> Option<Snapshot> {
    db.get_session_snapshot(id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
}

/// Session ids are `YYYYMMDD_HHMMSS` in UTC.
fn started_from_id(id: &str) -> Option<String> {
    chrono::NaiveDateTime::parse_from_str(id, "%Y%m%d_%H%M%S")
        .ok()
        .map(|t| t.and_utc().to_rfc3339())
}

fn rfc3339(unix: u64) -> Option<String> {
    chrono::DateTime::from_timestamp(unix as i64, 0).map(|t| t.to_rfc3339())
}

/// Everything worth showing about one unfinished recording.
fn describe(
    db: &Database,
    id: &str,
    record: Option<&SessionRecord>,
    dir: &Path,
    segments: &[SegmentReport],
) -> Option<serde_json::Value> {
    let snap = snapshot(db, id);
    let tracks: Vec<_> = segments.iter().flat_map(|s| s.tracks.iter()).collect();
    let broken = tracks.iter().filter(|t| !t.header_ok).count();
    let has_temp = segments.iter().any(|s| !s.temp_files.is_empty());
    let uncompressed = segments
        .iter()
        .any(|s| !s.tracks.is_empty() && !s.has_playback);
    let wanted_compression = snap
        .as_ref()
        .is_some_and(|s| s.recording.enabled && s.recording.auto_compress);

    let mut reasons = Vec::new();
    match record.map(|r| r.state.as_str()) {
        Some("recording") | Some("paused") => {
            reasons.push("Gravai closed while this was still recording.".to_string())
        }
        None if !tracks.is_empty() => {
            reasons.push("The audio is on disk but the recording is not in your library.".into())
        }
        _ => {}
    }
    if broken > 0 {
        reasons.push(format!(
            "{broken} audio track{} {} never finished, so players see {} as empty.",
            if broken == 1 { "" } else { "s" },
            if broken == 1 { "was" } else { "were" },
            if broken == 1 { "it" } else { "them" },
        ));
    }
    if has_temp {
        reasons.push("Compressing the audio was interrupted.".into());
    }
    if uncompressed && wanted_compression && broken == 0 && !has_temp {
        reasons.push("The raw tracks were never compressed into a playback file.".into());
    }
    if reasons.is_empty() {
        return None;
    }

    let utterances = db.get_utterances(id).unwrap_or_default();
    let last_spoken = utterances
        .iter()
        .filter_map(|u| u.end_ms)
        .max()
        .map(|ms| ms as f64 / 1000.0)
        .unwrap_or(0.0);
    let seconds = recovery::timeline_seconds(segments).max(last_spoken);
    let preview: Vec<_> = utterances
        .iter()
        .take(3)
        .chain(utterances.iter().skip(3).last())
        .map(|u| {
            serde_json::json!({
                "speaker": u.speaker.clone().unwrap_or_else(|| u.source.clone()),
                "text": u.corrected_text.clone().unwrap_or_else(|| u.text.clone()),
                "start_ms": u.start_ms,
            })
        })
        .collect();
    let raw_bytes: u64 = segments.iter().map(|s| s.raw_bytes()).sum();
    let bitrate = snap
        .as_ref()
        .map(|s| s.recording.aac_bitrate_kbps)
        .unwrap_or(192);
    let last_write = segments.iter().filter_map(|s| s.last_write).max();

    Some(serde_json::json!({
        "session_id": id,
        "title": record.and_then(|r| r.title.clone()),
        "started_at": record.map(|r| r.started_at.clone()).or_else(|| started_from_id(id)),
        "last_write": last_write.and_then(rfc3339),
        "meeting_app": record.and_then(|r| r.meeting_app.clone()),
        "db_state": record.map(|r| r.state.clone()),
        "in_library": record.is_some(),
        "reasons": reasons,
        "seconds": seconds,
        "dir": dir.display().to_string(),
        "segments": segments.len(),
        "tracks": tracks.iter().map(|t| serde_json::json!({
            "name": t.path.file_stem().map(|s| s.to_string_lossy().to_string()),
            "seconds": t.seconds,
            "bytes": t.file_bytes,
            "header_ok": t.header_ok,
            "sample_rate": t.sample_rate,
            "channels": t.channels,
        })).collect::<Vec<_>>(),
        "raw_bytes": raw_bytes,
        "has_playback": segments.iter().any(|s| s.has_playback),
        "convert_bytes": (seconds * f64::from(bitrate) * 1000.0 / 8.0) as u64,
        "utterances": utterances.len(),
        "preview": preview,
        "has_summary": db.get_session_summary(id).ok().flatten().is_some(),
        "bookmarks": db.list_bookmarks(id).map(|b| b.len()).unwrap_or(0),
        "settings": snap.as_ref().map(|s| s.summary()),
    }))
}

/// Recordings that did not finish cleanly, newest first.
#[tauri::command]
pub async fn scan_incomplete_sessions(
    state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    let active = state.session.read().await.as_ref().map(|s| s.id.clone());
    tokio::task::spawn_blocking(move || {
        let db = super::session::shared_db()?;
        let records: HashMap<String, SessionRecord> = db
            .list_sessions()
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();

        let mut candidates: Vec<(String, PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(gravai_config::sessions_dir()) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    let id = entry.file_name().to_string_lossy().to_string();
                    let dir = audio_dir(&db, &id);
                    candidates.push((id, dir));
                }
            }
        }
        // Sessions left "recording" whose audio went to a custom folder.
        for record in records.values() {
            if record.state != "stopped" && !candidates.iter().any(|(id, _)| id == &record.id) {
                candidates.push((record.id.clone(), audio_dir(&db, &record.id)));
            }
        }

        let mut found: Vec<serde_json::Value> = candidates
            .into_iter()
            .filter(|(id, _)| active.as_deref() != Some(id.as_str()))
            .filter_map(|(id, dir)| {
                let segments = recovery::inspect_session_dir(&dir);
                describe(&db, &id, records.get(&id), &dir, &segments)
            })
            .collect();
        found.sort_by(|a, b| b["started_at"].as_str().cmp(&a["started_at"].as_str()));
        if !found.is_empty() {
            info!("Found {} unfinished recording(s)", found.len());
        }
        Ok(serde_json::json!(found))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Finish one unfinished recording.
///
/// - `convert`: repair the tracks, compress them into `playback.m4a`, mark it finished.
/// - `keep`: repair the tracks and mark it finished, leaving the raw WAVs.
/// - `delete`: remove the audio and the library entry.
#[tauri::command]
pub async fn resolve_incomplete_session(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    action: String,
) -> Result<String, String> {
    if state.session.read().await.as_ref().map(|s| s.id.as_str()) == Some(session_id.as_str()) {
        return Err("That recording is still running".into());
    }
    let bitrate = state.config.read().await.audio.recording.aac_bitrate_kbps;
    tokio::task::spawn_blocking(move || {
        let db = super::session::shared_db()?;
        let id = session_id;
        let dir = audio_dir(&db, &id);
        let default_dir = gravai_config::sessions_dir().join(&id);
        let record = db.get_session(&id).map_err(|e| e.to_string())?;

        if action == "delete" {
            if dir == default_dir {
                let _ = std::fs::remove_dir_all(&dir);
            } else {
                // A shared custom folder: remove only this session's files.
                for segment in recovery::inspect_session_dir(&dir) {
                    for track in &segment.tracks {
                        let _ = std::fs::remove_file(&track.path);
                    }
                    for temp in &segment.temp_files {
                        let _ = std::fs::remove_file(temp);
                    }
                }
                let _ = std::fs::remove_dir_all(&default_dir);
            }
            db.delete_session(&id).map_err(|e| e.to_string())?;
            info!("Recovery: deleted {id}");
            return Ok("Deleted".to_string());
        }
        if action != "convert" && action != "keep" {
            return Err(format!("Unknown action: {action}"));
        }

        let repaired = recovery::repair_session_dir(&dir)?;
        let segments = recovery::inspect_session_dir(&dir);
        let mut message = if repaired > 0 {
            format!("Repaired {repaired} track(s)")
        } else {
            "Tracks were already intact".to_string()
        };
        if action == "convert" {
            let mut freed = 0u64;
            for segment in segments.iter().filter(|s| !s.tracks.is_empty()) {
                match gravai_audio::encoder::compact_session(&segment.dir, bitrate, true) {
                    Ok(Some(result)) => freed += result.freed_bytes,
                    Ok(None) => {}
                    Err(e) => {
                        warn!("Recovery: compress {} failed: {e}", segment.dir.display());
                        return Err(format!(
                            "{message}, but compressing failed (raw tracks kept): {e}"
                        ));
                    }
                }
            }
            message = format!(
                "{message} and compressed, freeing {:.0} MB",
                freed as f64 / 1_048_576.0
            );
        }

        let utterance_end = db
            .get_utterances(&id)
            .unwrap_or_default()
            .iter()
            .filter_map(|u| u.end_ms)
            .max()
            .map(|ms| ms as f64 / 1000.0)
            .unwrap_or(0.0);
        let seconds = recovery::timeline_seconds(&segments).max(utterance_end);
        let ended = segments
            .iter()
            .filter_map(|s| s.last_write)
            .max()
            .and_then(rfc3339)
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        match record {
            Some(r) => {
                let duration = r.duration_seconds.unwrap_or(0.0).max(seconds);
                db.update_session_state(&id, "stopped", Some(&ended), Some(duration))
                    .map_err(|e| e.to_string())?;
            }
            None => {
                db.create_session(&SessionRecord {
                    id: id.clone(),
                    started_at: started_from_id(&id).unwrap_or_else(|| ended.clone()),
                    ended_at: Some(ended),
                    duration_seconds: Some(seconds),
                    title: Some("Recovered recording".into()),
                    meeting_app: None,
                    state: "stopped".into(),
                })
                .map_err(|e| e.to_string())?;
            }
        }
        info!("Recovery: {action} {id}: {message}");
        Ok(message)
    })
    .await
    .map_err(|e| e.to_string())?
}
