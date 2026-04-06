//! Shared application state (Arc<AppState>).
//!
//! Ported from ears-rust-api app_state.rs, expanded with profile/preset fields.

use std::sync::Arc;
use tokio::sync::RwLock;

use gravai_config::AppConfig;

use crate::event_bus::EventBus;
use crate::session::Session;

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub event_bus: EventBus,
    pub session: RwLock<Option<Arc<Session>>>,
    pub active_profile: RwLock<Option<String>>,
    pub active_preset: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: RwLock::new(config),
            event_bus: EventBus::new(),
            session: RwLock::new(None),
            active_profile: RwLock::new(None),
            active_preset: RwLock::new(None),
        }
    }
}
