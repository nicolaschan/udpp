use std::{io::Error, sync::Arc, net::SocketAddr, future::Future, pin::Pin, task::{Poll, Context}, collections::HashSet};

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use snow::Keypair;
use thiserror::Error;
use tokio::{task::JoinHandle, sync::{Mutex, mpsc::{UnboundedSender, UnboundedReceiver, unbounded_channel}}, net::{UdpSocket, ToSocketAddrs}};

use crate::{transport::{AddressedSender, AddressedReceiver}, snow_types::{SnowKeypair, SnowPublicKey}, handler::Handler, session::{Session, SessionId}, ip_discovery::discover_ips, transform::{Bidirectional, Chunker}};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub addresses: HashSet<SocketAddr>,
    pub public_key: SnowPublicKey,
}

#[derive(Clone)]
pub struct VeqSocket {
    connection_info: ConnectionInfo,
    handler: Handler,
}

impl VeqSocket {
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> Result<VeqSocket, Error> {
        let socket = Arc::new(UdpSocket::bind(addrs).await?);
        let keypair = SnowKeypair::new();
        let connection_info = ConnectionInfo {
            addresses: discover_ips(&socket).await,
            public_key: keypair.public(),
        };
        let (handler, mut outgoing_receiver) = Handler::new(keypair);

        let receiver = socket.clone();
        let mut handler_recv = handler.clone();
        tokio::spawn(async move {
            loop {
                let mut buf: [u8; 65536] = [0u8; 65536];
                let (len, src) = receiver.recv_from(&mut buf).await.unwrap();
                handler_recv.handle_incoming(src, buf[..len].to_vec()).await;
            }
        });

        let handler_send = handler.clone();
        tokio::spawn(async move {
            loop {
                if let Some((dest, packet, ttl)) = outgoing_receiver.recv().await {
                    let serialized = bincode::serialize(&packet).unwrap();
                    socket.set_ttl(ttl).unwrap();
                    if let Ok(_) = socket.send_to(&serialized[..], dest).await {}
                }
            }
        });
        Ok(VeqSocket {
            connection_info,
            handler,
        })
    }

    pub fn connection_info(&self) -> ConnectionInfo {
        self.connection_info.clone()
    }

    pub fn is_initiator(&self, info: &ConnectionInfo) -> bool {
        self.connection_info.public_key < info.public_key
    }

    pub async fn connect(&mut self, id: SessionId, info: ConnectionInfo) -> VeqSessionAlias {
        {
            match self.is_initiator(&info) {
                true => self.handler.initiate(id, info).await,
                false => self.handler.respond(id, info).await,
            }
        }.await;
        let remote_addr = self.handler.remote_addr(id).await.unwrap();

        let delegate = BidirectionalSession { id, handler: self.handler.clone() };
        let delegate = Chunker::new(delegate, 1000);
        VeqSession { delegate, remote_addr }
    }
}

#[derive(Error, Debug)]
pub enum VeqError {
    #[error("Session disconnected")]
    Disconnected,
}

#[derive(Clone)]
pub struct BidirectionalSession {
    id: SessionId,
    handler: Handler,
}

#[async_trait]
impl Bidirectional for BidirectionalSession {
    async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError> {
        self.handler.send(self.id, data).await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>, VeqError> {
        let future = {
            self.handler.recv_from(self.id).await.unwrap()
        };
        future.await
    }
}

#[derive(Clone)]
pub struct VeqSession<T> {
    delegate: T,
    remote_addr: SocketAddr,
}

impl<T: Bidirectional> VeqSession<T> {
    pub async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError> {
        self.delegate.send(data).await
    }
    pub async fn recv(&mut self) -> Result<Vec<u8>, VeqError> {
        self.delegate.recv().await
    }
    pub async fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }
}

pub type VeqSessionAlias = VeqSession<Chunker<BidirectionalSession>>;