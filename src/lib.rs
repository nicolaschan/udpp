#![feature(map_first_last)]
#![feature(async_stream)]
#![feature(untagged_unions)]
#![feature(future_join)]
#![feature(future_poll_fn)]

use std::{net::SocketAddr, future::Future, pin::Pin, task::{Context, Poll}, sync::Arc};

use async_std::net::UdpSocket;
use async_trait::async_trait;
use proto::{data::{SnowKeypair, SnowPublicKey}, bidirectional::Bidirectional, coordinator::{Coordinator, PendingConnection}};
use snow::Keypair;

mod holepunch;
mod proto;
mod transport;

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    addresses: Vec<SocketAddr>,
    public_key: SnowPublicKey,
}

pub struct Peer {
    coordinator: Coordinator,
    connection_info: ConnectionInfo,
    keypair: SnowKeypair,
}

impl Peer {
    pub async fn new() -> Peer {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await.unwrap());
        let keypair = SnowKeypair::new();
        let connection_info = ConnectionInfo {
            addresses: vec![socket.local_addr().unwrap()],
            public_key: keypair.public(),
        };
        let coordinator = Coordinator::new(socket.clone(), socket);
        Peer { coordinator, connection_info, keypair }
    }

    pub fn connection_info(&self) -> ConnectionInfo {
        self.connection_info.clone()
    }

    pub fn connect(&mut self, peer_info: ConnectionInfo) -> PendingConnection {
        self.coordinator.connect(peer_info)
    }
}

#[cfg(test)]
mod tests {
    use tokio::join;

    use crate::{Peer, proto::bidirectional::Bidirectional};


    #[tokio::test]
    async fn test_handshake() {
        let mut peer1 = Peer::new().await;
        let mut peer2 = Peer::new().await;

        let peer1_info = peer1.connection_info();
        let peer2_info = peer2.connection_info();

        println!("{:?}", peer1_info);

        let (mut conn1, mut conn2) = join!(
            peer1.connect(peer2.connection_info()),
            peer2.connect(peer1.connection_info()),
        );

        let data: Vec<u8> = vec![1, 2, 3, 4];
        conn1.send(data.clone()).await;

        let received = conn2.recv().await;
        assert_eq!(data, received);

        // let mut initiator1 = SessionInitiator::new();
        // let mut initiator2 = SessionInitiator::new();

        // let initiation = initiator1.initiation();
        // let (session2, response) = initiator2.receive_initiation(initiation);

        // let session1 = initiator1.receive_response(response).unwrap();
    }
}
