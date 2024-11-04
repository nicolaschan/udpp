use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6, ToSocketAddrs},
};

use stunclient::StunClient;
use tokio::net::UdpSocket;

use network_interface::Addr;
use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;

use crate::dualstack::maybe_dual::MaybeDual;

pub fn local_ips_v4(port: u16) -> Result<HashSet<SocketAddr>, network_interface::Error> {
    let ifs = NetworkInterface::show()?;
    Ok(ifs
        .into_iter()
        .flat_map(|f| f.addr)
        .filter_map(|addr| match addr {
            Addr::V4(addr_v4) => Some(addr_v4),
            _ => None,
        })
        .map(|ip| SocketAddrV4::new(ip.ip, port).into())
        .collect())
}

pub fn local_ips_v6(port: u16) -> Result<HashSet<SocketAddr>, network_interface::Error> {
    log::debug!("Starting local ips v6");
    let ifs = NetworkInterface::show()?;
    Ok(ifs
        .into_iter()
        .flat_map(|f| f.addr)
        .filter_map(|addr| match addr {
            Addr::V6(addr_v6) => Some(addr_v6),
            _ => None,
        })
        .map(|ip| SocketAddrV6::new(ip.ip, port, 0, 0).into())
        .collect())
}

pub fn local_ips(port: u16) -> Result<HashSet<SocketAddr>, network_interface::Error> {
    let ifs = NetworkInterface::show()?;
    Ok(ifs
        .into_iter()
        .flat_map(|f| f.addr)
        .map(|ip| SocketAddr::new(ip.ip(), port).into())
        .collect())
}

pub async fn public_v4(socket: &UdpSocket) -> HashSet<SocketAddr> {
    let mut ips = HashSet::new();
    if let Some(stun_addr_v4) = "stun.l.google.com:19302"
        .to_socket_addrs()
        .ok()
        .and_then(|mut iter| iter.find(|x| x.is_ipv4()))
    {
        let client = StunClient::new(stun_addr_v4);
        match client.query_external_address_async(socket).await {
            Ok(addr) => {
                ips.insert(addr);
                // let mut ip_with_port = addr.clone();
                // ip_with_port.set_port(socket.local_addr().unwrap().port());
                // ips.insert(ip_with_port);
            }
            Err(e) => {
                log::debug!("Failed to get IPv4 addresses from STUN: {:?}", e);
            }
        }
    }
    log::debug!("IPv4 addrs from STUN: {:?}", ips);
    ips
}

pub async fn public_v6(socket: &UdpSocket) -> Result<HashSet<SocketAddr>, std::io::Error> {
    let mut ips = HashSet::new();
    if let Some(stun_addr_v6) = "stun.l.google.com:19302"
        .to_socket_addrs()
        .ok()
        .and_then(|mut iter| iter.find(|x| x.is_ipv6()))
    {
        let client = StunClient::new(stun_addr_v6);
        match client.query_external_address_async(socket).await {
            Ok(addr) => {
                ips.insert(addr);
                let mut ip_with_port = addr;
                ip_with_port.set_port(socket.local_addr()?.port());
                ips.insert(ip_with_port);
            }
            Err(e) => {
                log::debug!("Failed to get IPv6 addresses from STUN: {:?}", e);
            }
        }
    }
    log::debug!("IPv6 addrs from STUN: {:?}", ips);
    Ok(ips)
}

pub async fn discover_ips(
    socket: &impl MaybeDual<SocketT = UdpSocket>,
) -> anyhow::Result<HashSet<SocketAddr>> {
    let mut ips = HashSet::new();

    if let Some(v4_socket) = socket.v4() {
        let local_addr = v4_socket.local_addr()?;
        ips.insert(local_addr);
        let port = local_addr.port();
        ips.extend(&local_ips_v4(port)?);

        let public_addrs = public_v4(v4_socket).await;
        ips.extend(public_addrs);
    }

    if let Some(v6_socket) = socket.v6() {
        let local_addr = v6_socket.local_addr()?;
        ips.insert(local_addr);
        let port = local_addr.port();
        ips.extend(&local_ips_v6(port)?);

        let public_addrs = public_v6(v6_socket).await;
        if let Ok(ipv6_addrs) = public_addrs {
            ips.extend(&ipv6_addrs);
        }
    }

    ips.retain(|x: &SocketAddr| !IpAddr::is_unspecified(&x.ip()));

    Ok(ips)
}

#[cfg(test)]
mod tests {
    use crate::dualstack::versioned_socket::VersionedSocket;

    use super::discover_ips;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

    #[tokio::test]
    async fn test_ifs() {
        let udp_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let port = udp_socket.local_addr().unwrap().port();

        let socket = VersionedSocket::V4(udp_socket);
        let ips = discover_ips(&socket).await.unwrap();

        assert!(!ips.contains(&format!("0.0.0.0:{}", port).parse::<SocketAddr>().unwrap()));
        assert!(ips.contains(&format!("127.0.0.1:{}", port).parse::<SocketAddr>().unwrap()));
    }
}
