use std::{
    io,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
};

use super::{bidir_socket::BidirSocket, maybe_dual::MaybeDual, versioned_socket::VersionedSocket};

pub struct DualSocket<V4SocketT: BidirSocket, V6SocketT: BidirSocket> {
    v4_socket: V4SocketT,
    v6_socket: V6SocketT,
}

impl<V4SocketT: BidirSocket, V6SocketT: BidirSocket> DualSocket<V4SocketT, V6SocketT> {
    pub fn new(v4_socket: V4SocketT, v6_socket: V6SocketT) -> Self {
        Self {
            v4_socket,
            v6_socket,
        }
    }
}

impl DualSocket<VersionedSocket, VersionedSocket> {
    pub async fn dualstack(
        v4_socket_addr: SocketAddrV4,
        v6_socket_addr: SocketAddrV6,
    ) -> io::Result<Self> {
        let (v4_socket, v6_socket) = tokio::join!(
            VersionedSocket::v4(v4_socket_addr),
            VersionedSocket::v6(v6_socket_addr)
        );
        let (v4_socket, v6_socket) = (v4_socket?, v6_socket?);
        Ok(Self::new(v4_socket, v6_socket))
    }
}

impl<V4SocketT: BidirSocket + Sync, V6SocketT: BidirSocket + Sync> BidirSocket
    for DualSocket<V4SocketT, V6SocketT>
{
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        // TODO: Reuse buffers to reallocating
        let mut v4_buf = vec![0; buf.len()];
        let mut v6_buf = vec![0; buf.len()];

        // recv_from is cancel-safe, so if one of the recv_from calls completes first, the other
        // is guaranteed not to have received any data.
        tokio::select! {
            res = self.v4_socket.recv_from(&mut v4_buf[..]) => {
                if let Ok((size, _addr)) = res {
                    buf.copy_from_slice(&v4_buf[..size]);
                }
                res
            },
            res = self.v6_socket.recv_from(&mut v6_buf[..]) => {
                if let Ok((size, _addr)) = res {
                    buf.copy_from_slice(&v6_buf[..size]);
                }
                res
            },
        }
    }

    fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.v4_socket.set_ttl(ttl)?;
        self.v6_socket.set_ttl(ttl)?;
        Ok(())
    }

    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        match addr {
            SocketAddr::V4(_) => self.v4_socket.send_to(buf, addr).await,
            SocketAddr::V6(_) => self.v6_socket.send_to(buf, addr).await,
        }
    }
}

impl<SocketT: BidirSocket + MaybeDual> MaybeDual for DualSocket<SocketT, SocketT> {
    type SocketT = SocketT::SocketT;

    fn v4(&self) -> Option<&Self::SocketT> {
        self.v4_socket.v4()
    }

    fn v6(&self) -> Option<&Self::SocketT> {
        self.v6_socket.v6()
    }
}
