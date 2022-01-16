use std::{net::{SocketAddr, IpAddr, ToSocketAddrs}, str::FromStr, collections::HashSet};

use pnet::datalink;
use stunclient::StunClient;
use tokio::net::UdpSocket;

pub fn local_ips(port: u16) -> HashSet<SocketAddr> {
    let ifs = datalink::interfaces();
    return ifs.into_iter()
        .flat_map(|f| f.ips)
        .map(|ip| SocketAddr::new(ip.ip(), port))
        .collect();
}

pub async fn public_v4(socket: &UdpSocket) -> HashSet<SocketAddr> {
    let mut ips = HashSet::new();
    if let Some(stun_addr_v4) = "stun.l.google.com:19302".to_socket_addrs().unwrap().filter(|x|x.is_ipv4()).next() {
        let client = StunClient::new(stun_addr_v4);
        if let Ok(addr) = client.query_external_address_async(&socket).await {
            ips.insert(addr);
            // let mut ip_with_port = addr.clone();
            // ip_with_port.set_port(socket.local_addr().unwrap().port());
            // ips.insert(ip_with_port);
        }
    }
    ips
}

pub async fn public_v6(socket: &UdpSocket) -> HashSet<SocketAddr> {
    let mut ips = HashSet::new();
    if let Some(stun_addr_v4) = "stun.l.google.com:19302".to_socket_addrs().unwrap().filter(|x|x.is_ipv4()).next() {
        let client = StunClient::new(stun_addr_v4);
        if let Ok(addr) = client.query_external_address_async(&socket).await {
            ips.insert(addr);
            // let mut ip_with_port = addr.clone();
            // ip_with_port.set_port(socket.local_addr().unwrap().port());
            // ips.insert(ip_with_port);
        }
    }
    ips
}

pub async fn discover_ips(socket: &UdpSocket) -> HashSet<SocketAddr> {
    let mut ips = HashSet::new();
    let local_addr = socket.local_addr().unwrap();
    let port = local_addr.port();

    ips.insert(local_addr);
    ips.extend(&local_ips(port));
    ips.extend(&public_v4(socket).await);
    ips.extend(&public_v6(socket).await);

    return ips;
}

mod tests {
    use tokio::net::UdpSocket;

    use crate::ip_discovery::discover_ips;

    #[tokio::test]
    async fn test_ifs() {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let ips = discover_ips(&socket).await;
        println!("{:?}", ips);
        assert!(ips.contains(&socket.local_addr().unwrap()));
    }
}