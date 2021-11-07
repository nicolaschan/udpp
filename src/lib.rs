#![feature(map_first_last)]
use crossbeam::channel::unbounded;

use rand::RngCore;
use crossbeam::channel::{Sender, Receiver};
use std::net::SocketAddr;

use std::sync::Arc;
use std::sync::Mutex;


use serde::{Deserialize, Serialize};
use std::net::UdpSocket;


use std::result::Result;
use std::io::Error;
use std::collections::HashMap;

pub mod proto;
pub mod transport;

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, net::{SocketAddr, ToSocketAddrs}};

    use crossbeam::channel::unbounded;

    use crate::{proto::{handler::UdppHandler, socket::{UdppServer, UdppSession}}};

    fn socket_addr(string: &str) -> SocketAddr {
        string
            .to_socket_addrs()
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    // #[test]
    // fn handles_empty_data() {
    //     let (sender1, receiver1) = unbounded();
    //     let (sender2, receiver2) = unbounded();

    //     let mut handler1: UdppHandler = UdppHandler::new(Box::new(sender1));
    //     let mut handler2: UdppHandler = UdppHandler::new(Box::new(sender2));


    //     let addr = socket_addr("127.0.0.1:8080");
    //     handler1.establish_session(addr);

    //     let (_addr, received ) = receiver1.recv().unwrap();
    //     println!("{:?}", received);

    //     handler2.handle_incoming(addr, received);
    //     let (_addr, received2 ) = receiver2.recv().unwrap();
    //     println!("{:?}", received2);

    //     println!("{:?}", handler1.sessions);
    //     println!("{:?}", handler2.sessions);

    //     handler1.handle_incoming(addr, received2);
    //     println!("{:?}", handler1.sessions);
    //     println!("{:?}", handler2.sessions);
    // }

    #[tokio::test]
    async fn test_server() {
        let (sender1, receiver1) = unbounded();
        let (sender2, receiver2) = unbounded();

        let mut server1 = UdppServer::new(Box::new(sender1), Box::new(receiver2));
        let mut conn = UdppSession::new(Box::new(sender2), Box::new(receiver1), socket_addr("127.0.0.1:8080")).await;

        println!("{:?}", server1);
        println!("{:?}", conn);
    }
}