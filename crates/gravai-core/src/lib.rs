//! Gravai core: AppState, EventBus, Session FSM, error types, preflight, logging.

pub mod app_state;
pub mod error;
pub mod event_bus;
pub mod logging;
pub mod preflight;
pub mod session;

pub use app_state::AppState;
pub use error::{GravaiError, Result};
pub use event_bus::{EventBus, GravaiEvent};
pub use session::{Session, SessionState};
