use std::{error::Error, net::{SocketAddr, UdpSocket}, sync::{Arc, Mutex}};

use crossbeam::channel::{Receiver, Sender};



pub trait AddressedSender {
    fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error>;
}

pub trait AddressedReceiver {
    fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error>;
}

impl AddressedSender for Sender<(SocketAddr, Vec<u8>)> {
    fn addressed_send(&mut self, address: SocketAddr, payload: Vec<u8>) -> Result<usize, std::io::Error> {
        let len = payload.len();
        self.send((address, payload)).unwrap();
        Ok(len)
    }
}

impl AddressedReceiver for Receiver<(SocketAddr, Vec<u8>)> {
    fn addressed_recv(&mut self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        Ok(self.recv().unwrap())
    }
}
