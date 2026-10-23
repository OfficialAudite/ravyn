use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Url;

/// The client for webhooks, which post to URLs any user can set. Unless
/// `allow_private` is on, it refuses to connect anywhere on the server's
/// own network, at resolve time so a hostname that later flips to an
/// internal address is caught too. Redirects are off, since one could
/// point at an IP literal and skip the resolver.
pub fn webhook_client(allow_private: bool) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none());
    if !allow_private {
        builder = builder.dns_resolver(Arc::new(PublicOnlyResolver));
    }
    builder.build().expect("failed to build http client")
}

/// IP literals never reach the resolver, so they're checked separately.
pub fn is_allowed_target(url: &str, allow_private: bool) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    if allow_private {
        return true;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => is_public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_public(IpAddr::V6(ip)),
        Some(url::Host::Domain(_)) => true,
        None => false,
    }
}

struct PublicOnlyResolver;

impl Resolve for PublicOnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(
                    format!("{} does not resolve to a public address", name.as_str()).into(),
                );
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 198 && (18..20).contains(&b))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        || first == 0x2001 && ip.segments()[1] == 0xdb8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_internal_targets() {
        for url in [
            "http://127.0.0.1/hook",
            "http://10.0.0.5/",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://0.0.0.0/",
        ] {
            assert!(!is_allowed_target(url, false), "{url}");
            assert!(is_allowed_target(url, true), "{url}");
        }
    }

    #[test]
    fn allows_public_targets() {
        assert!(is_allowed_target(
            "https://discord.com/api/webhooks/1/x",
            false
        ));
        assert!(is_allowed_target("http://1.1.1.1/", false));
        assert!(is_allowed_target("http://[2606:4700::1111]/", false));
        assert!(!is_allowed_target("not a url", false));
    }
}
