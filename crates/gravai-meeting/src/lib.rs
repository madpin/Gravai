//! Meeting detection and calendar integration.
//!
//! Monitors running processes for known meeting apps and emits detection events.
//! Reads calendar events to auto-name recording sessions.

pub mod calendar;
pub mod detector;
