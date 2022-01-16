use std::{io::Error, sync::Arc, net::SocketAddr, future::Future, pin::Pin, task::{Poll, Context}};

use async_std::net::{ToSocketAddrs, UdpSocket};
use serde::{Serialize, Deserialize};
use snow::Keypair;
use tokio::{task::JoinHandle, sync::Mutex};

use crate::{transport::{AddressedSender, AddressedReceiver}, snow_types::{SnowKeypair, SnowPublicKey}, handler::Handler, session::{Session, SessionId}};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub addresses: Vec<SocketAddr>,
    pub public_key: SnowPublicKey,
}

pub struct VeqSocket {
    connection_info: ConnectionInfo,
    keypair: SnowKeypair,
    handler: Arc<Mutex<Handler>>,
    receiving_handle: JoinHandle<()>,
    sending_handle: JoinHandle<()>,
}

impl VeqSocket {
    pub async fn bind<A: ToSocketAddrs>(addrs: A) -> Result<VeqSocket, Error> {
        let socket = Arc::new(UdpSocket::bind(addrs).await?);
        let keypair = SnowKeypair::new();
        let connection_info = ConnectionInfo {
            addresses: vec![socket.local_addr()?],
            public_key: keypair.public(),
        };
        let handler = Arc::new(Mutex::new(Handler::new()));

        let receiver = socket.clone();
        let handler_recv = handler.clone();
        let receiving_handle = tokio::spawn(async move {
            loop {
                let mut buf: [u8; 65536] = [0u8; 65536];
                let (len, src) = receiver.recv_from(&mut buf).await.unwrap();
                let mut guard = handler_recv.lock().await;
                guard.handle_incoming(src, buf[..len].to_vec()).await;
            }
        });

        let handler_send = handler.clone();
        let sending_handle = tokio::spawn(async move {
            loop {
                let mut guard = handler_send.lock().await;
                if let Some((dest, data)) = guard.try_next_outgoing().await {
                    socket.send_to(&data[..], dest).await.unwrap();
                }
            }
        });
        Ok(VeqSocket {
            connection_info,
            keypair,
            handler,
            receiving_handle,
            sending_handle,
        })
    }

    pub fn connection_info(&self) -> ConnectionInfo {
        self.connection_info.clone()
    }

    pub fn is_initiator(&self, info: &ConnectionInfo) -> bool {
        self.connection_info.public_key < info.public_key
    }

    pub async fn connect(&mut self, id: SessionId, info: ConnectionInfo) -> VeqSession {
        {
            let mut guard = self.handler.lock().await;
            match self.is_initiator(&info) {
                true => guard.initiate(id, info).await,
                false => guard.respond(id, info).await,
            }
        }.await;
        VeqSession { id, handler: self.handler.clone() }
    }
}

#[derive(Clone)]
pub struct VeqSession {
    id: SessionId,
    handler: Arc<Mutex<Handler>>,
}

impl VeqSession {
    pub async fn send(&mut self, data: Vec<u8>) {
        let mut guard = self.handler.lock().await;
        guard.send(self.id, data).await;
    }
    pub async fn recv(&mut self) -> Vec<u8> {
        let future = {
            let mut guard = self.handler.lock().await;
            guard.recv_from(self.id).await.unwrap()
        };
        future.await
    }
}