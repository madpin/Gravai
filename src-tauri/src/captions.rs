//! Native window/tray adapter; transcript state lives in gravai-core.
use gravai_config::live_accessibility::{CaptionMode, CaptionPosition, LiveCaptions};
use gravai_core::captions::CaptionBuffer;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct Captions {
    config: LiveCaptions,
    buffer: CaptionBuffer,
    hidden: bool,
}

pub fn configure(app: &tauri::AppHandle, config: &LiveCaptions) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Captions>>();
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if s.config.enabled != config.enabled {
            s.hidden = false;
        }
        s.config = config.clone();
        if !config.enabled {
            s.buffer.lines.clear();
        }
    }
    sync_window(app)?;
    publish(app);
    Ok(())
}
fn sync_window(app: &tauri::AppHandle) -> Result<(), String> {
    let (c, hidden) = {
        let state = app.state::<Mutex<Captions>>();
        let s = state.lock().map_err(|e| e.to_string())?;
        (s.config.clone(), s.hidden)
    };
    let show = c.enabled && c.mode != CaptionMode::MenuBar && !hidden;
    if !show {
        if let Some(w) = app.get_webview_window("captions") {
            w.hide().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let window = match app.get_webview_window("captions") {
        Some(w) => w,
        None => tauri::WebviewWindowBuilder::new(
            app,
            "captions",
            tauri::WebviewUrl::App("index.html?captions=1".into()),
        )
        .transparent(true)
        .title("Gravai captions")
        .decorations(false)
        .resizable(false)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .build()
        .map_err(|e| e.to_string())?,
    };
    window
        .set_always_on_top(c.always_on_top)
        .map_err(|e| e.to_string())?;
    window.set_shadow(c.shadow).map_err(|e| e.to_string())?;
    if let Some(monitor) = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
    {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let width = (c.width_px as f64).min(area.size.width as f64 / scale - 32.0);
        let height = ((c.font_pt as f64 * (96.0 / 72.0) * 1.5 * c.max_lines as f64) + 52.0)
            .min(area.size.height as f64 / scale - 32.0);
        let left = area.position.x as f64 / scale + 16.0;
        let top = area.position.y as f64 / scale + 16.0;
        let right = (area.position.x as f64 + area.size.width as f64) / scale - width - 16.0;
        let bottom = (area.position.y as f64 + area.size.height as f64) / scale - height - 16.0;
        let (x, y) = match c.position {
            CaptionPosition::TopLeft => (left, top),
            CaptionPosition::TopRight => (right, top),
            CaptionPosition::BottomLeft => (left, bottom),
            CaptionPosition::BottomRight => (right, bottom),
        };
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
        window
            .set_position(tauri::LogicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())
}
fn publish(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<Captions>>();
    let Ok(s) = state.lock() else {
        return;
    };
    let _ = app.emit_to(
        "captions",
        "gravai:captions",
        serde_json::json!({"config": s.config, "buffer": s.buffer}),
    );
    if let Some(tray) = app.tray_by_id("gravai-tray") {
        let latest = s.buffer.lines.last().map(String::as_str).unwrap_or("");
        let menu = s.config.enabled && s.config.mode != CaptionMode::Overlay;
        let title = if menu {
            latest
                .chars()
                .take(s.config.menu_bar_max_chars)
                .collect::<String>()
        } else {
            String::new()
        };
        let _ = tray.set_title(Some(&title));
        let _ = tray.set_tooltip(Some(if menu && !latest.is_empty() {
            latest
        } else {
            "Gravai"
        }));
    }
}
pub fn event(app: &tauri::AppHandle, event: &gravai_core::GravaiEvent) {
    {
        let state = app.state::<Mutex<Captions>>();
        let Ok(mut s) = state.lock() else {
            return;
        };
        match event {
            gravai_core::GravaiEvent::SessionStateChanged { state, session_id } => {
                s.buffer.session(session_id.as_deref(), state)
            }
            gravai_core::GravaiEvent::TranscriptUpdated {
                session_id, text, ..
            } if s.config.enabled => s.buffer.push(session_id, text),
            _ => return,
        }
    }
    publish(app);
}
#[tauri::command]
pub fn get_live_captions(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let state = app.state::<Mutex<Captions>>();
    let s = state.lock().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"config": s.config, "buffer": s.buffer}))
}
#[tauri::command]
pub fn toggle_caption_overlay(app: tauri::AppHandle) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Captions>>();
        let mut s = state.lock().map_err(|e| e.to_string())?;
        if !s.config.enabled {
            return Ok(());
        }
        s.hidden = !s.hidden;
    }
    sync_window(&app)
}
