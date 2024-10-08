use std::{
    collections::HashSet,
    net::{SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::net::UdpSocket;

use crate::{
    dualstack::{
        bidir_socket::BidirSocket, dual_socket::DualSocket, maybe_dual::MaybeDual,
        versioned_socket::VersionedSocket,
    },
    handler::{Handler, OneTimeId},
    ip_discovery::discover_ips,
    session::SessionId,
    snow_types::{SnowKeypair, SnowPublicKey},
    transform::{Bidirectional, Chunker},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub addresses: HashSet<SocketAddr>,
    pub public_key: SnowPublicKey,
}

#[derive(Clone)]
pub struct VeqSocket {
    connection_info: ConnectionInfo,
    keypair: SnowKeypair,
    handler: Handler,
}

impl VeqSocket {
    pub async fn bind(addr: &str) -> anyhow::Result<VeqSocket> {
        let keypair = SnowKeypair::new()?;
        let socket = VersionedSocket::bind(addr).await?;
        VeqSocket::from_socket(socket, keypair).await
    }

    pub async fn bind_with_keypair(addr: &str, keypair: SnowKeypair) -> anyhow::Result<VeqSocket> {
        let socket = VersionedSocket::bind(addr).await?;
        VeqSocket::from_socket(socket, keypair).await
    }

    pub async fn dualstack(v4_addr: &str, v6_addr: &str) -> anyhow::Result<VeqSocket> {
        let keypair = SnowKeypair::new()?;
        let socket_addr_v4 = v4_addr.parse()?;
        let socket_addr_v6 = v6_addr.parse()?;
        VeqSocket::dualstack_with_keypair(socket_addr_v4, socket_addr_v6, keypair).await
    }

    pub async fn dualstack_with_keypair(
        socket_addr_v4: SocketAddrV4,
        socket_addr_v6: SocketAddrV6,
        keypair: SnowKeypair,
    ) -> anyhow::Result<VeqSocket> {
        let socket = DualSocket::dualstack(socket_addr_v4, socket_addr_v6).await?;
        VeqSocket::from_socket(socket, keypair).await
    }

    pub async fn from_socket(
        socket: impl BidirSocket + MaybeDual<SocketT = UdpSocket> + Send + Sync + 'static,
        keypair: SnowKeypair,
    ) -> anyhow::Result<VeqSocket> {
        let connection_info = ConnectionInfo {
            addresses: discover_ips(&socket).await?,
            public_key: keypair.public(),
        };

        let (handler, mut outgoing_receiver) = Handler::new(keypair.clone());

        let socket = Arc::new(socket);
        let receiver = socket.clone();
        let mut handler_recv = handler.clone();
        tokio::spawn(async move {
            loop {
                let mut buf: [u8; 65536] = [0u8; 65536];

                match receiver.recv_from(&mut buf).await {
                    Ok((len, src)) => {
                        if let Err(e) = handler_recv.handle_incoming(src, buf[..len].to_vec()).await
                        {
                            log::debug!("VeqSocket Handler handle_incoming failed with {e}");
                        }
                    }
                    Err(e) => {
                        log::warn!("Could not receive packet from UDP socket: {}", e);
                    }
                };
            }
        });

        tokio::spawn(async move {
            loop {
                if let Some((dest, packet, ttl)) = outgoing_receiver.recv().await {
                    let serialized = bincode::serialize(&packet);
                    if let Err(ref e) = &serialized {
                        log::debug!("Failed to serialize packet: {e}");
                        continue;
                    }
                    let serialized = serialized.unwrap();
                    if let Err(e) = socket.set_ttl(ttl) {
                        log::debug!("Failed to set TTL on socket: {e}");
                        // continue;
                    }
                    if let Err(_e) = socket.send_to(&serialized[..], dest).await {
                        log::error!("Failed to send packet to {}", dest);
                    }
                } else {
                    log::warn!("outgoing_receiver channel closed");
                }
            }
        });
        Ok(VeqSocket {
            connection_info,
            keypair,
            handler,
        })
    }

    pub fn connection_info(&self) -> ConnectionInfo {
        self.connection_info.clone()
    }

    pub fn is_initiator(&self, info: &ConnectionInfo) -> bool {
        self.connection_info.public_key < info.public_key
    }

    pub fn keypair(&self) -> SnowKeypair {
        self.keypair.clone()
    }

    pub async fn connect(
        &mut self,
        id: SessionId,
        info: ConnectionInfo,
    ) -> anyhow::Result<VeqSessionAlias> {
        let (id, one_time_id) = {
            match self.is_initiator(&info) {
                true => self.handler.initiate(id, info).await?,
                false => self.handler.respond(id, info).await?,
            }
        }
        .await;
        let remote_addr = self
            .handler
            .remote_addr(id)
            .await
            .ok_or(anyhow::anyhow!("Failed to get remote addr"))?;

        let delegate = Arc::new(BidirectionalSession {
            id,
            one_time_id,
            handler: self.handler.clone(),
        });
        let delegate = Chunker::new(delegate, 1000);
        Ok(VeqSession {
            delegate,
            remote_addr,
        })
    }
}

#[derive(Error, Debug)]
pub enum VeqError {
    #[error("Session disconnected")]
    Disconnected,
}

pub struct BidirectionalSession {
    id: SessionId,
    one_time_id: OneTimeId,
    handler: Handler,
}

#[async_trait]
impl Bidirectional for Arc<BidirectionalSession> {
    async fn send(&mut self, data: Vec<u8>) -> Result<(), VeqError> {
        self.handler
            .send(self.id, data)
            .await
            .map_err(|_e| VeqError::Disconnected)?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<Vec<u8>, VeqError> {
        let future = {
            self.handler
                .recv_from(self.id)
                .await
                .ok_or(VeqError::Disconnected)?
        };
        future.await
    }
}

impl Drop for BidirectionalSession {
    fn drop(&mut self) {
        self.handler.close_session(self.id, self.one_time_id);
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

pub type VeqSessionAlias = VeqSession<Chunker<Arc<BidirectionalSession>>>;
