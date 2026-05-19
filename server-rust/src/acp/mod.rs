//! ACP bridge — WebSocket chat routed through Agent Client Protocol agents.

#[cfg(feature = "acp-bridge")]
pub mod bridge;
#[cfg(feature = "acp-bridge")]
pub mod config;
#[cfg(feature = "acp-bridge")]
pub mod mapper;
#[cfg(feature = "acp-bridge")]
pub mod permissions;
#[cfg(feature = "acp-bridge")]
pub mod registry;
#[cfg(feature = "acp-bridge")]
pub mod runtime;
#[cfg(feature = "acp-bridge")]
pub mod session_handle;

#[cfg(feature = "acp-bridge")]
pub use bridge::AcpBridge;

/// Whether the ACP bridge path is active at runtime.
pub fn acp_enabled() -> bool {
    crate::acp::config::acp_bridge_enabled()
}
