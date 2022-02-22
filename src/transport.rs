use std::{net::SocketAddr, sync::Arc};

use async_std::{
    channel::{Receiver, Sender},
    net::UdpSocket,
    sync::Mutex,
};
use async_trait::async_trait;

#[async_trait]
pub trait AddressedSender {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>);
}

#[async_trait]
impl AddressedSender for Sender<(SocketAddr, Vec<u8>)> {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) {
        self.send((addr, data)).await.unwrap();
    }
}

#[async_trait]
impl AddressedSender for Arc<Mutex<Vec<(SocketAddr, Vec<u8>)>>> {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) {
        let mut guard = self.lock().await;
        guard.push((addr, data));
    }
}

#[async_trait]
impl AddressedSender for Arc<UdpSocket> {
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) {
        self.send_to(&data[..], addr).await.unwrap();
    }
}

#[async_trait]
pub trait AddressedReceiver {
    async fn recv(&self) -> (SocketAddr, Vec<u8>);
}

#[async_trait]
impl AddressedReceiver for Receiver<(SocketAddr, Vec<u8>)> {
    async fn recv(&self) -> (SocketAddr, Vec<u8>) {
        self.recv().await.unwrap()
    }
}

#[async_trait]
impl AddressedReceiver for Arc<UdpSocket> {
    async fn recv(&self) -> (SocketAddr, Vec<u8>) {
        let mut buf: [u8; 65536] = [0; 65536];
        let (size, addr) = self.recv_from(&mut buf).await.unwrap();
        (addr, buf[..size].to_vec())
    }
}
