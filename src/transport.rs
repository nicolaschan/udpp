use async_std::channel::{Receiver, Sender};
use async_trait::async_trait;

use std::{error::Error, net::{SocketAddr, UdpSocket}, sync::{Arc, Mutex}};


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
        Ok(len)
    }
}

#[async_trait]
impl AddressedReceiver for Receiver<(SocketAddr, Vec<u8>)> {
    async fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        Ok(self.recv().await.unwrap())
    }
}
