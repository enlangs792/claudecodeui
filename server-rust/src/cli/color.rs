// ANSI color codes for terminal output — mirrors server/cli.js colors

pub const RESET: &str = "\x1b[0m";
pub const BRIGHT: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const CYAN: &str = "\x1b[36m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const BLUE: &str = "\x1b[34m";

pub fn info(text: &str) -> String {
    format!("{CYAN}{text}{RESET}")
}
pub fn ok(text: &str) -> String {
    format!("{GREEN}{text}{RESET}")
}
pub fn warn(text: &str) -> String {
    format!("{YELLOW}{text}{RESET}")
}
pub fn error(text: &str) -> String {
    format!("{RED}{text}{RESET}")
}
pub fn tip(text: &str) -> String {
    format!("{BLUE}{text}{RESET}")
}
pub fn bright(text: &str) -> String {
    format!("{BRIGHT}{text}{RESET}")
}
pub fn dim(text: &str) -> String {
    format!("{DIM}{text}{RESET}")
}
