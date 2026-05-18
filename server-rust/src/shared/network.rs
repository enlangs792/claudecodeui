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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wildcard_hosts() {
        assert!(is_wildcard_host("0.0.0.0"));
        assert!(is_wildcard_host("::"));
        assert!(!is_wildcard_host("127.0.0.1"));
        assert!(!is_wildcard_host("192.168.1.1"));
    }

    #[test]
    fn test_loopback_hosts() {
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("example.com"));
    }

    #[test]
    fn test_connectable_host_returns_localhost_for_wildcard() {
        assert_eq!(get_connectable_host("0.0.0.0"), "localhost");
        assert_eq!(get_connectable_host("::"), "localhost");
        assert_eq!(get_connectable_host(""), "localhost");
    }

    #[test]
    fn test_connectable_host_returns_localhost_for_loopback() {
        assert_eq!(get_connectable_host("localhost"), "localhost");
        assert_eq!(get_connectable_host("127.0.0.1"), "localhost");
    }

    #[test]
    fn test_connectable_host_passes_through_other_hosts() {
        assert_eq!(get_connectable_host("example.com"), "example.com");
        assert_eq!(get_connectable_host("192.168.1.1"), "192.168.1.1");
    }
}
