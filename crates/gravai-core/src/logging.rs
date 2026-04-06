//! Logging infrastructure: ring buffer + file + stderr.
//!
//! Ported from ears-rust-api log_capture.rs, adapted for Gravai paths.

use std::collections::VecDeque;
use std::sync::Mutex;

use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Maximum number of log lines kept in the ring buffer.
const MAX_LOG_LINES: usize = 200;

static LOG_BUFFER: Mutex<Option<VecDeque<String>>> = Mutex::new(None);

fn ensure_buffer() -> &'static Mutex<Option<VecDeque<String>>> {
    let mut buf = LOG_BUFFER.lock().unwrap();
    if buf.is_none() {
        *buf = Some(VecDeque::with_capacity(MAX_LOG_LINES));
    }
    drop(buf);
    &LOG_BUFFER
}

/// A `tracing_subscriber::Layer` that captures log lines into an in-memory ring buffer.
pub struct RingBufferLayer;

impl Default for RingBufferLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl RingBufferLayer {
    pub fn new() -> Self {
        ensure_buffer();
        RingBufferLayer
    }
}

impl<S: Subscriber> Layer<S> for RingBufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = metadata.level();

        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let now = chrono::Local::now();
        let line = format!("[{}] {} {}", now.format("%H:%M:%S"), level, message);

        if let Ok(mut guard) = LOG_BUFFER.lock() {
            if let Some(buf) = guard.as_mut() {
                if buf.len() >= MAX_LOG_LINES {
                    buf.pop_front();
                }
                buf.push_back(line);
            }
        }
    }
}

/// Returns the most recent log lines from the ring buffer.
pub fn recent_logs() -> Vec<String> {
    if let Ok(guard) = LOG_BUFFER.lock() {
        if let Some(buf) = guard.as_ref() {
            return buf.iter().cloned().collect();
        }
    }
    Vec::new()
}

/// Clears all log lines from the ring buffer.
pub fn clear_logs() {
    if let Ok(mut guard) = LOG_BUFFER.lock() {
        if let Some(buf) = guard.as_mut() {
            buf.clear();
        }
    }
}

/// Initialize the tracing subscriber with stderr + file + ring buffer layers.
pub fn init_logging() {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::EnvFilter;

    let log_path = gravai_config::log_file_path();
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or(std::path::Path::new(".")),
        log_path.file_name().unwrap_or_default(),
    );

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false),
        )
        .with(RingBufferLayer::new())
        .init();
}

/// Field visitor that extracts the `message` field from a tracing event.
struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;
        if field.name() == "message" {
            let _ = write!(self.0, "{:?}", value);
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            let _ = write!(self.0, "{}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        use std::fmt::Write;
        if field.name() == "message" {
            let _ = write!(self.0, "{}", value);
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            let _ = write!(self.0, "{}={}", field.name(), value);
        }
    }
}
