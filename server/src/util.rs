pub(crate) mod dns {
    use std::net::IpAddr;

    use anyhow::Result;
    use hickory_resolver::{Resolver, net::NetError};

    pub async fn resolve_domain(domain: &str) -> Result<Option<IpAddr>> {
        // Use the host OS'es `/etc/resolv.conf`
        let resolver = Resolver::builder_tokio()?.build()?;
        let response = match resolver.lookup_ip(domain).await {
            Ok(ip) => ip,
            Err(NetError::Dns(hickory_resolver::net::DnsError::NoRecordsFound(_))) => {
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };

        Ok(response.iter().filter(|i| i.is_ipv4()).next())
    }
}
