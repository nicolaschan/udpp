use async_std::{channel::{Receiver, Sender}, net::UdpSocket};
use async_trait::async_trait;

use std::{net::{SocketAddr}, sync::Arc};


#[async_trait]
pub trait AddressedSender {
    async fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error>;
}

#[async_trait]
pub trait AddressedReceiver {
    async fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error>;
}

#[async_trait]
impl AddressedSender for Sender<(SocketAddr, Vec<u8>)> {
    async fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error> {
        let len = payload.len();
        self.send((address, payload)).await.unwrap();
        println!("sent");
        Ok(len)
    }
}

#[async_trait]
impl AddressedReceiver for Receiver<(SocketAddr, Vec<u8>)> {
    async fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        let result = Ok(self.recv().await.unwrap());
        println!("recv");
        result
    }
}

#[async_trait]
impl AddressedSender for Arc<UdpSocket> {
    async fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error> {
        Ok(self.send_to(&payload[..], address).await.unwrap())
    }
}

#[async_trait]
impl AddressedReceiver for Arc<UdpSocket> {
    async fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        let mut buf = [0u8; 65536];
        let (length, addr) = self.recv_from(&mut buf).await.unwrap();
        Ok((addr, buf[..length].to_vec()))
    }
}