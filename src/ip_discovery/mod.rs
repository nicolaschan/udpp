use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddr, ToSocketAddrs},
};

use stunclient::StunClient;
use tokio::net::UdpSocket;

use network_interface::NetworkInterface;
use network_interface::NetworkInterfaceConfig;

pub fn local_ips(port: u16) -> Result<HashSet<SocketAddr>, network_interface::Error> {
    let ifs = NetworkInterface::show()?;
    Ok(ifs
        .into_iter()
        .flat_map(|f| f.addr)
        .map(|ip| SocketAddr::new(ip.ip(), port))
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
        if let Ok(addr) = client.query_external_address_async(socket).await {
            ips.insert(addr);
            // let mut ip_with_port = addr.clone();
            // ip_with_port.set_port(socket.local_addr().unwrap().port());
            // ips.insert(ip_with_port);
        }
    }
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
        if let Ok(addr) = client.query_external_address_async(socket).await {
            ips.insert(addr);
            let mut ip_with_port = addr;
            ip_with_port.set_port(socket.local_addr()?.port());
            ips.insert(ip_with_port);
        }
    }
    Ok(ips)
}

pub async fn discover_ips(socket: &UdpSocket) -> anyhow::Result<HashSet<SocketAddr>> {
    let mut ips = HashSet::new();
    let local_addr = socket.local_addr()?;

    ips.insert(local_addr);
    let port = local_addr.port();
    ips.extend(&local_ips(port)?);

    ips.extend(&public_v4(socket).await);
    ips.extend(&public_v6(socket).await?);

    ips.retain(|x| x.ip() != Ipv4Addr::new(0, 0, 0, 0));

    Ok(ips)
}

#[cfg(test)]
mod tests {
    use super::discover_ips;
    use std::net::SocketAddr;
    use tokio::net::UdpSocket;

    #[tokio::test]
    async fn test_ifs() {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let ips = discover_ips(&socket).await.unwrap();

        let port = socket.local_addr().unwrap().port();

        assert!(!ips.contains(&format!("0.0.0.0:{}", port).parse::<SocketAddr>().unwrap()));
        assert!(ips.contains(&format!("127.0.0.1:{}", port).parse::<SocketAddr>().unwrap()));
    }
}
