//! Network helpers — mirrors shared/networkHosts.js

/// True for wildcard bind addresses like 0.0.0.0 and ::
pub fn is_wildcard_host(host: &str) -> bool {
    host == "0.0.0.0" || host == "::"
}

/// True for loopback addresses
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Returns "localhost" for loopback/wildcard, otherwise the host unchanged
pub fn get_connectable_host(host: &str) -> String {
    if host.is_empty() || is_wildcard_host(host) || is_loopback_host(host) {
        "localhost".into()
    } else {
        host.into()
    }
}
