use std::{future::Future, io, net::SocketAddr};

pub trait BidirSocket {
    fn recv_from(
        &self,
        buf: &mut [u8],
    ) -> impl Future<Output = io::Result<(usize, SocketAddr)>> + Send;
    fn set_ttl(&self, ttl: u32) -> io::Result<()>;
    fn send_to(
        &self,
        buf: &[u8],
        addr: SocketAddr,
    ) -> impl Future<Output = io::Result<usize>> + Send;
}
