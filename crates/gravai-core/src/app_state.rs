//! Shared application state (Arc<AppState>).
//!
//! Ported from ears-rust-api app_state.rs.

use std::sync::Arc;
use tokio::sync::RwLock;

use gravai_config::AppConfig;

use crate::event_bus::EventBus;
use crate::session::Session;

pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub event_bus: EventBus,
    pub session: RwLock<Option<Arc<Session>>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: RwLock::new(config),
            event_bus: EventBus::new(),
            session: RwLock::new(None),
        }
    }
}
