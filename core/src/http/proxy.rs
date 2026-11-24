use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IpNet {
    addr: IpAddr,
    prefix: u8,
}

impl IpNet {
    pub fn parse(text: &str) -> Option<IpNet> {
        let (addr_text, prefix_text) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text, None),
        };

        let addr = addr_text.parse::<IpAddr>().ok()?.to_canonical();
        let max = max_prefix(addr);

        let prefix = match prefix_text {
            Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                value.parse::<u8>().ok()?
            }
            Some(_) => return None,
            None => max,
        };

        if prefix > max {
            return None;
        }

        return Some(IpNet {
            addr: mask(addr, prefix),
            prefix,
        });
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();

        if ip.is_ipv4() != self.addr.is_ipv4() {
            return false;
        }

        return mask(ip, self.prefix) == self.addr;
    }
}

fn max_prefix(addr: IpAddr) -> u8 {
    return if addr.is_ipv4() { 32 } else { 128 };
}

fn mask(addr: IpAddr, prefix: u8) -> IpAddr {
    return match addr {
        IpAddr::V4(ip) => {
            let bits = u32::from(ip);
            let kept = if prefix == 0 {
                0
            } else {
                bits & (u32::MAX << (32 - prefix))
            };
            IpAddr::V4(kept.into())
        }
        IpAddr::V6(ip) => {
            let bits = u128::from(ip);
            let kept = if prefix == 0 {
                0
            } else {
                bits & (u128::MAX << (128 - prefix))
            };
            IpAddr::V6(kept.into())
        }
    };
}

fn is_trusted(ip: IpAddr, trusted: &[IpNet]) -> bool {
    return trusted.iter().any(|net| net.contains(ip));
}

pub fn resolve_client<'h>(
    peer: IpAddr,
    forwarded: impl Iterator<Item = &'h str>,
    trusted: &[IpNet],
) -> IpAddr {
    let mut client = peer.to_canonical();

    if !is_trusted(client, trusted) {
        return client;
    }

    let values: Vec<&str> = forwarded.collect();

    for entry in values.iter().rev().flat_map(|value| value.rsplit(',')) {
        if !is_trusted(client, trusted) {
            break;
        }

        match entry.trim().parse::<IpAddr>() {
            Ok(ip) => client = ip.to_canonical(),
            Err(_) => break,
        }
    }

    return client;
}

#[cfg(test)]
mod tests {
    use super::{resolve_client, IpNet};
    use std::net::IpAddr;

    fn nets(list: &[&str]) -> Vec<IpNet> {
        return list
            .iter()
            .map(|text| IpNet::parse(text).unwrap())
            .collect();
    }

    fn client(peer: &str, forwarded: &[&str], trusted: &[&str]) -> String {
        let peer = peer.parse::<IpAddr>().unwrap();
        return resolve_client(peer, forwarded.iter().copied(), &nets(trusted)).to_string();
    }

    #[test]
    fn forwarded_needs_a_trusted_proxy() {
        assert_eq!(client("127.0.0.1", &["9.9.9.9"], &["127.0.0.1"]), "9.9.9.9");
        assert_eq!(
            client("127.0.0.1", &["2606:4700:4700::1111"], &["127.0.0.1"]),
            "2606:4700:4700::1111"
        );
        assert_eq!(client("1.2.3.4", &["9.9.9.9"], &["127.0.0.1"]), "1.2.3.4");
        assert_eq!(client("1.2.3.4", &["9.9.9.9"], &[]), "1.2.3.4");
        assert_eq!(client("127.0.0.1", &["9.9.9.9"], &[]), "127.0.0.1");
    }

    #[test]
    fn bad_forwarded_stops_at_the_last_trusted() {
        assert_eq!(client("127.0.0.1", &[], &["127.0.0.1"]), "127.0.0.1");
        assert_eq!(client("127.0.0.1", &[""], &["127.0.0.1"]), "127.0.0.1");
        assert_eq!(
            client("127.0.0.1", &["<script>"], &["127.0.0.1"]),
            "127.0.0.1"
        );
        assert_eq!(
            client("127.0.0.1", &["1.2.3.4:80"], &["127.0.0.1"]),
            "127.0.0.1"
        );
        assert_eq!(
            client("127.0.0.1", &["fe80::1%eth0"], &["127.0.0.1"]),
            "127.0.0.1"
        );
        assert_eq!(
            client("127.0.0.1", &["9.9.9.9, "], &["127.0.0.1"]),
            "127.0.0.1"
        );
    }

    #[test]
    fn forwarded_is_read_from_the_right() {
        assert_eq!(
            client("127.0.0.1", &["1.1.1.1, 2.2.2.2"], &["127.0.0.1"]),
            "2.2.2.2"
        );
        assert_eq!(
            client("127.0.0.1", &["<script>, 2.2.2.2"], &["127.0.0.1"]),
            "2.2.2.2"
        );
        assert_eq!(
            client("127.0.0.1", &["1.1.1.1,2.2.2.2"], &["127.0.0.1"]),
            "2.2.2.2"
        );
    }

    #[test]
    fn chain_of_trusted_proxies() {
        let trusted = ["127.0.0.1", "10.0.0.0/8"];

        assert_eq!(
            client("127.0.0.1", &["1.1.1.1, 9.9.9.9, 10.1.2.3"], &trusted),
            "9.9.9.9"
        );
        assert_eq!(
            client("127.0.0.1", &["10.0.0.2, 10.1.2.3"], &trusted),
            "10.0.0.2"
        );
        assert_eq!(
            client("127.0.0.1", &["1.1.1.1", "9.9.9.9, 10.1.2.3"], &trusted),
            "9.9.9.9"
        );
        assert_eq!(
            client("127.0.0.1", &["9.9.9.9", "10.1.2.3"], &trusted),
            "9.9.9.9"
        );
    }

    #[test]
    fn subnets() {
        assert_eq!(
            client("10.20.30.40", &["9.9.9.9"], &["10.0.0.0/8"]),
            "9.9.9.9"
        );
        assert_eq!(
            client("11.0.0.1", &["9.9.9.9"], &["10.0.0.0/8"]),
            "11.0.0.1"
        );
        assert_eq!(client("fd00::5", &["9.9.9.9"], &["fd00::/8"]), "9.9.9.9");
        assert_eq!(client("fe00::5", &["9.9.9.9"], &["fd00::/8"]), "fe00::5");
        assert_eq!(client("5.6.7.8", &["9.9.9.9"], &["0.0.0.0/0"]), "9.9.9.9");
        assert_eq!(client("::1", &["9.9.9.9"], &["0.0.0.0/0"]), "::1");
        assert_eq!(client("::1", &["9.9.9.9"], &["::/0"]), "9.9.9.9");
    }

    #[test]
    fn mapped_addresses() {
        assert_eq!(
            client("::ffff:127.0.0.1", &["9.9.9.9"], &["127.0.0.1"]),
            "9.9.9.9"
        );
        assert_eq!(client("::ffff:1.2.3.4", &[], &[]), "1.2.3.4");
        assert_eq!(
            client("127.0.0.1", &["::ffff:9.9.9.9"], &["127.0.0.1"]),
            "9.9.9.9"
        );
        assert_eq!(
            client("127.0.0.1", &["9.9.9.9"], &["::ffff:127.0.0.1"]),
            "9.9.9.9"
        );
    }

    #[test]
    fn parse_nets() {
        assert_eq!(IpNet::parse("10.1.2.3/8"), IpNet::parse("10.0.0.0/8"));
        assert!(IpNet::parse("1.2.3.4").is_some());
        assert!(IpNet::parse("1.2.3.4/32").is_some());
        assert!(IpNet::parse("::/128").is_some());
        assert!(IpNet::parse("1.2.3.4/33").is_none());
        assert!(IpNet::parse("::/129").is_none());
        assert!(IpNet::parse("1.2.3.4/").is_none());
        assert!(IpNet::parse("1.2.3.4/+8").is_none());
        assert!(IpNet::parse("1.2.3.4/8/8").is_none());
        assert!(IpNet::parse("abc").is_none());
        assert!(IpNet::parse("").is_none());
    }
}
