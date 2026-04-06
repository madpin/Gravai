//! Startup health checks.
//!
//! Ported from ears-rust-api preflight.rs, adapted for Gravai.

use serde::Serialize;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize)]
pub struct HealthCheck {
    pub name: String,
    pub status: String, // "ok", "warn", "error"
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub checks: Vec<HealthCheck>,
    pub overall: String,
}

/// Run all preflight checks. Returns a HealthReport.
pub fn run_preflight_checks(config: &gravai_config::AppConfig) -> HealthReport {
    let checks = vec![
        check_platform(),
        check_storage(),
        check_audio_devices(),
        check_transcription_model(config),
    ];

    let overall = if checks.iter().any(|c| c.status == "error") {
        "error"
    } else if checks.iter().any(|c| c.status == "warn") {
        "warn"
    } else {
        "ok"
    }
    .to_string();

    let report = HealthReport { checks, overall };
    log_preflight_report(&report);
    report
}

fn check_platform() -> HealthCheck {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;

    if os != "macos" {
        return HealthCheck {
            name: "platform".into(),
            status: "error".into(),
            message: format!("Unsupported OS: {os} (requires macOS)"),
        };
    }

    if arch != "aarch64" {
        return HealthCheck {
            name: "platform".into(),
            status: "warn".into(),
            message: format!(
                "Running on {arch}; Apple Silicon (aarch64) recommended for best performance"
            ),
        };
    }

    HealthCheck {
        name: "platform".into(),
        status: "ok".into(),
        message: format!("macOS {arch}"),
    }
}

fn check_storage() -> HealthCheck {
    let dir = gravai_config::data_dir();
    match std::fs::create_dir_all(&dir) {
        Ok(_) => {
            // Test write
            let test_file = dir.join(".preflight_test");
            match std::fs::write(&test_file, "ok") {
                Ok(_) => {
                    let _ = std::fs::remove_file(&test_file);
                    HealthCheck {
                        name: "storage".into(),
                        status: "ok".into(),
                        message: format!("Data dir writable: {}", dir.display()),
                    }
                }
                Err(e) => HealthCheck {
                    name: "storage".into(),
                    status: "error".into(),
                    message: format!("Cannot write to {}: {e}", dir.display()),
                },
            }
        }
        Err(e) => HealthCheck {
            name: "storage".into(),
            status: "error".into(),
            message: format!("Cannot create {}: {e}", dir.display()),
        },
    }
}

fn check_audio_devices() -> HealthCheck {
    use cpal::traits::HostTrait;

    let host = cpal::default_host();
    let device_count = host.input_devices().map(|d| d.count()).unwrap_or(0);

    if device_count == 0 {
        HealthCheck {
            name: "audio_devices".into(),
            status: "error".into(),
            message: "No audio input devices found".into(),
        }
    } else {
        HealthCheck {
            name: "audio_devices".into(),
            status: "ok".into(),
            message: format!("{device_count} input device(s) found"),
        }
    }
}

fn check_transcription_model(config: &gravai_config::AppConfig) -> HealthCheck {
    let model_name = &config.transcription.model;
    let model_path = gravai_config::models_dir().join(format!("ggml-{model_name}.bin"));

    if model_path.exists() {
        let size = std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0);
        HealthCheck {
            name: "transcription_model".into(),
            status: "ok".into(),
            message: format!(
                "Whisper {model_name} present ({:.0} MB)",
                size as f64 / 1_048_576.0
            ),
        }
    } else {
        HealthCheck {
            name: "transcription_model".into(),
            status: "warn".into(),
            message: format!(
                "Whisper {model_name} not found at {}; will download on first use",
                model_path.display()
            ),
        }
    }
}

fn log_preflight_report(report: &HealthReport) {
    info!("Preflight: overall={}", report.overall);
    for check in &report.checks {
        match check.status.as_str() {
            "ok" => info!("  [ok] {}: {}", check.name, check.message),
            "warn" => warn!("  [warn] {}: {}", check.name, check.message),
            _ => warn!("  [error] {}: {}", check.name, check.message),
        }
    }
}
