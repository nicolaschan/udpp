use async_std::channel::{Receiver, Sender};
use async_trait::async_trait;

use std::{net::{SocketAddr}};


#[async_trait]
pub trait AddressedSender {
    async fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error>;
}

#[async_trait]
pub trait AddressedReceiver {
    async fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error>;
    fn addressed_try_recv(&mut self) -> Option<(SocketAddr, Vec<u8>)>;
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
    fn addressed_try_recv(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        self.try_recv().ok()
    }
}
