use std::{error::Error, net::{SocketAddr, UdpSocket}};

use crossbeam::channel::{Receiver, Sender};
use std::net::ToSocketAddrs;

pub trait Transport {
    fn recv(&self) -> Result<(SocketAddr, Vec<u8>), std::io::Error>;
    fn send(&self, addr: SocketAddr, data: Vec<u8>) -> Result<usize, std::io::Error>;
}

pub trait Sink {
    fn send(&self, addr: SocketAddr, data: Vec<u8>) -> Result<usize, std::io::Error>;
}

pub struct ChannelSink {
    pub sender: Sender<Vec<u8>>,
}

impl ChannelSink {
    pub fn new(sender: Sender<Vec<u8>>) -> ChannelSink {
        ChannelSink { sender }
    }
}

impl Sink for ChannelSink {
    fn send(&self, _addr: SocketAddr, data: Vec<u8>) -> Result<usize, std::io::Error> {
        let len = data.len();
        self.sender.send(data).unwrap();
        Ok(len)
    }
}

pub struct UdpTransport {
    udp_socket: UdpSocket,
}

impl Transport for UdpTransport {
    fn recv(&self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        let mut buf = [0; 65536];
        let (len, src) = self.udp_socket.recv_from(&mut buf)?;
        Ok((src, buf[..len].to_vec()))
    }

    fn send(&self, addr: SocketAddr, data: Vec<u8>) -> Result<usize, std::io::Error> {
        self.udp_socket.send_to(&data[..], addr)
    }
}

pub struct ChannelTransport {
    send_channel: Sender<Vec<u8>>,
    recv_channel: Receiver<Vec<u8>>,
}

impl ChannelTransport {
    pub fn new() -> ChannelTransport {
        let (send_channel, recv_channel) = crossbeam::channel::unbounded();
        ChannelTransport { send_channel, recv_channel }
    }
}

impl Transport for ChannelTransport {
    fn recv(&self) -> Result<(SocketAddr, Vec<u8>), std::io::Error> {
        let addr = "127.0.0.1:8080"
            .to_socket_addrs()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let received = self.recv_channel.recv().unwrap();
        Ok((addr, received))
    }

    fn send(&self, _addr: SocketAddr, data: Vec<u8>) -> Result<usize, std::io::Error> {
        let len = data.len();
        self.send_channel.send(data).unwrap();
        Ok(len)
    }
}

pub fn transport_pair() -> (ChannelTransport, ChannelTransport) {
    let (sender1, receiver1) = crossbeam::channel::unbounded();
    let (sender2, receiver2) = crossbeam::channel::unbounded();
    let transport1 = ChannelTransport {
        send_channel: sender1,
        recv_channel: receiver2,
    };
    let transport2 = ChannelTransport {
        send_channel: sender2,
        recv_channel: receiver1,
    };
    (transport1, transport2)
}