//! Pure address comparison for restart preflight; does not open sockets.
use std::net::SocketAddr;

pub(super) fn conflicting_peer(targets: &[SocketAddr], peers: &[(u32, SocketAddr)], own_pid: u32) -> Option<u32> {
    peers.iter().find_map(|(pid, bound)| {
        (*pid != own_pid && targets.iter().any(|target| {
            target.port() == bound.port()
                && (target.ip() == bound.ip()
                    || (target.is_ipv4() == bound.is_ipv4()
                        && (target.ip().is_unspecified() || bound.ip().is_unspecified())))
        })).then_some(*pid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn address(value: &str) -> SocketAddr { value.parse().unwrap() }
    #[test]
    fn preserves_self_restart_and_distinct_ports() {
        let peers = [(10, address("127.0.0.1:46000")), (20, address("127.0.0.1:46001"))];
        assert_eq!(conflicting_peer(&[address("127.0.0.1:46000")], &peers, 10), None);
        assert_eq!(conflicting_peer(&[address("127.0.0.1:46002")], &peers, 10), None);
        assert_eq!(conflicting_peer(&[address("127.0.0.1:46001")], &peers, 10), Some(20));
    }
    #[test]
    fn detects_wildcard_and_resolved_alias_addresses() {
        let peers = [(20, address("127.0.0.1:46001"))];
        assert_eq!(conflicting_peer(&[address("0.0.0.0:46001")], &peers, 10), Some(20));
        assert_eq!(conflicting_peer(&[address("[::1]:46001"), address("127.0.0.1:46001")], &peers, 10), Some(20));
        assert_eq!(conflicting_peer(&[address("127.0.0.2:46001")], &peers, 10), None);
    }
}
