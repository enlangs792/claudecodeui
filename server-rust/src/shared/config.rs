/// Configuration constants mirrored from server/constants/config.js
pub const IS_PLATFORM: bool = option_env!("IS_PLATFORM").is_some();
pub const SERVER_PORT_DEFAULT: u16 = 3001;
pub const VITE_PORT_DEFAULT: u16 = 5173;
pub const HOST_DEFAULT: &str = "0.0.0.0";
