use std::{
    io,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
};

use tokio::net::UdpSocket;

use super::{bidir_socket::BidirSocket, maybe_dual::MaybeDual};

pub enum VersionedSocket {
    V4(UdpSocket),
    V6(UdpSocket),
}

impl VersionedSocket {
    pub async fn bind(addr: &str) -> io::Result<Self> {
        match addr.parse() {
            Ok(SocketAddr::V4(socket_addr)) => Self::v4(socket_addr).await,
            Ok(SocketAddr::V6(socket_addr)) => Self::v6(socket_addr).await,
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid address",
            )),
        }
    }

    pub async fn v4(socket_addr: SocketAddrV4) -> io::Result<Self> {
        Ok(VersionedSocket::V4(UdpSocket::bind(socket_addr).await?))
    }

    pub async fn v6(socket_addr: SocketAddrV6) -> io::Result<Self> {
        Ok(VersionedSocket::V6(UdpSocket::bind(socket_addr).await?))
    }
}

impl BidirSocket for VersionedSocket {
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, std::net::SocketAddr)> {
        match self {
            VersionedSocket::V4(socket) => socket.recv_from(buf).await,
            VersionedSocket::V6(socket) => socket.recv_from(buf).await,
        }
    }

    fn set_ttl(&self, ttl: u32) -> std::io::Result<()> {
        match self {
            VersionedSocket::V4(socket) => socket.set_ttl(ttl),
            VersionedSocket::V6(socket) => socket.set_ttl(ttl),
        }
    }

    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> std::io::Result<usize> {
        match self {
            VersionedSocket::V4(socket) => socket.send_to(buf, addr).await,
            VersionedSocket::V6(socket) => socket.send_to(buf, addr).await,
        }
    }
}

impl MaybeDual for VersionedSocket {
    type SocketT = UdpSocket;

    fn v4(&self) -> Option<&Self::SocketT> {
        match self {
            VersionedSocket::V4(socket) => Some(socket),
            VersionedSocket::V6(_) => None,
        }
    }

    fn v6(&self) -> Option<&Self::SocketT> {
        match self {
            VersionedSocket::V4(_) => None,
            VersionedSocket::V6(socket) => Some(socket),
        }
    }
}
