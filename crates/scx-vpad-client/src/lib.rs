//! Unprivileged SCX virtual-pad client. The pipe owns a device lease; reports go
//! directly to that device's vendor HID collection, never through the service.
#[cfg(windows)]
pub mod channel;
#[cfg(windows)]
mod client;
#[cfg(windows)]
mod security;
#[cfg(windows)]
pub mod win;
pub mod wire;
#[cfg(windows)]
pub use client::{device_present, service_status, FeedbackReader, GamepadReader, VirtualPad};
