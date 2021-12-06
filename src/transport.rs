use std::{net::SocketAddr, sync::Arc};

use async_std::{channel::Sender, sync::Mutex};
use async_trait::async_trait;

#[async_trait]
pub trait AddressedSender {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>);
}

#[async_trait]
impl AddressedSender for Sender<(SocketAddr, Vec<u8>)> {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) {
        self.send((addr, data)).await;
    }
}

#[async_trait]
impl AddressedSender for Arc<Mutex<Vec<(SocketAddr, Vec<u8>)>>> {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) {
        let mut guard = self.lock().await;
        guard.push((addr, data));
    }
}
