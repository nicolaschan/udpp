use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
    time::Duration,
};

use crossbeam::channel::{unbounded, Receiver, Sender};
use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle, time};
use uuid::Uuid;

use crate::{
    snow_types::{LossyTransportState, SnowInitiation, SnowInitiator, SnowResponder, SnowResponse},
    veq::{ConnectionInfo, VeqError},
};

pub type SessionId = Uuid;

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionPacket {
    pub id: SessionId,
    pub data: Vec<u8>,
}

impl SessionPacket {
    pub fn new(id: SessionId, data: Vec<u8>) -> SessionPacket {
        SessionPacket { id, data }
    }
}

#[derive(Deserialize, Serialize)]
pub enum RawMessage {
    HandshakePoke,
    HandshakeInitiation(SnowInitiation),
    HandshakeResponse(SnowResponse),
    Encrypted(Vec<u8>),
}

#[derive(Debug, Deserialize, Serialize)]
pub enum Message {
    Keepalive,
    Payload(Vec<u8>),
}

pub struct PendingSessionPoker {
    handle: JoinHandle<()>,
    working_remote_addr: Option<(SocketAddr, SnowInitiation)>,
    waker: Arc<Mutex<Option<Waker>>>,
    id: SessionId,
    sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    responder: SnowResponder,
}

impl PendingSessionPoker {
    pub async fn new(
        sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
        id: SessionId,
        conn_info: ConnectionInfo,
        responder: SnowResponder,
    ) -> PendingSessionPoker {
        let addresses = conn_info.addresses;
        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                let serialized_message = bincode::serialize(&RawMessage::HandshakePoke).unwrap();
                addresses.clone().into_iter().for_each(|addr| {
                    sender_clone
                        .send((addr, SessionPacket::new(id, serialized_message.clone()), 64))
                        .unwrap();
                });
            }
        });
        PendingSessionPoker {
            handle,
            working_remote_addr: None,
            waker: Arc::new(Mutex::new(None)),
            id,
            sender,
            responder,
        }
    }

    pub async fn handle_incoming(&mut self, src: SocketAddr, packet: &SessionPacket) {
        if let Ok(RawMessage::HandshakeInitiation(initiation)) =
            bincode::deserialize::<RawMessage>(&packet.data)
        {
            self.working_remote_addr = Some((src, initiation));
            let mut guard = self.waker.lock().unwrap();
            if let Some(waker) = guard.take() {
                waker.wake_by_ref();
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.working_remote_addr.is_some()
    }

    pub async fn responder(self) -> Option<PendingSessionResponder> {
        if let Some((addr, initiation)) = self.working_remote_addr {
            self.handle.abort();
            let (transport, response) = self.responder.response(initiation).ok()?;
            return Some(
                PendingSessionResponder::new(
                    self.sender.clone(),
                    self.id,
                    addr,
                    transport,
                    response,
                )
                .await,
            );
        }
        None
    }
}

pub struct PendingSessionInitiator {
    handle: JoinHandle<()>,
    working_remote_addr: Option<(SocketAddr, SnowResponse)>,
    waker: Arc<Mutex<Option<Waker>>>,
    id: SessionId,
    sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    initiator: SnowInitiator,
}

impl PendingSessionInitiator {
    pub async fn new(
        sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
        id: SessionId,
        conn_info: ConnectionInfo,
        initiator: SnowInitiator,
    ) -> PendingSessionInitiator {
        let addresses = conn_info.addresses;
        let sender_clone = sender.clone();
        let initiation = initiator.initiation();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                let serialized_message =
                    bincode::serialize(&RawMessage::HandshakeInitiation(initiation.clone()))
                        .unwrap();
                addresses.clone().into_iter().for_each(|addr| {
                    sender_clone
                        .send((addr, SessionPacket::new(id, serialized_message.clone()), 64))
                        .unwrap();
                });
            }
        });
        PendingSessionInitiator {
            handle,
            working_remote_addr: None,
            waker: Arc::new(Mutex::new(None)),
            id,
            sender,
            initiator,
        }
    }

    pub async fn handle_incoming(&mut self, src: SocketAddr, packet: &SessionPacket) {
        if let Ok(RawMessage::HandshakeResponse(response)) =
            bincode::deserialize::<RawMessage>(&packet.data)
        {
            self.working_remote_addr = Some((src, response));
            let mut guard = self.waker.lock().unwrap();
            if let Some(waker) = guard.take() {
                waker.wake_by_ref();
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.working_remote_addr.is_some()
    }

    pub async fn session(self) -> Option<Session> {
        if let Some((remote_addr, response)) = self.working_remote_addr {
            self.handle.abort();
            let transport = self.initiator.receive_response(response);
            return Some(Session::new(
                self.id,
                remote_addr,
                self.sender.clone(),
                transport,
            ));
        }
        None
    }
}

pub struct PendingSessionResponder {
    handle: JoinHandle<()>,
    remote_addr: SocketAddr,
    waker: Arc<Mutex<Option<Waker>>>,
    id: SessionId,
    sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    transport: LossyTransportState,
    ready: bool,
}

impl PendingSessionResponder {
    pub async fn new(
        sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
        id: SessionId,
        remote_addr: SocketAddr,
        transport: LossyTransportState,
        response: SnowResponse,
    ) -> PendingSessionResponder {
        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                let serialized_message =
                    bincode::serialize(&RawMessage::HandshakeResponse(response.clone())).unwrap();
                sender_clone
                    .send((
                        remote_addr,
                        SessionPacket::new(id, serialized_message.clone()),
                        64,
                    ))
                    .unwrap();
            }
        });
        PendingSessionResponder {
            handle,
            remote_addr,
            waker: Arc::new(Mutex::new(None)),
            id,
            sender,
            transport,
            ready: false,
        }
    }

    pub async fn handle_incoming(&mut self, _src: SocketAddr, packet: &SessionPacket) {
        if let Ok(RawMessage::Encrypted(encrypted)) =
            bincode::deserialize::<RawMessage>(&packet.data)
        {
            let mut decrypted = [0u8; 65535];
            if let Ok(len) = self
                .transport
                .read_message(&encrypted, &mut decrypted)
                .await
            {
                if let Ok(Message::Keepalive) = bincode::deserialize(&decrypted[..len]) {
                    self.ready = true;
                    let mut guard = self.waker.lock().unwrap();
                    if let Some(waker) = guard.take() {
                        waker.wake_by_ref();
                    }
                }
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub async fn session(self) -> Option<Session> {
        if self.ready {
            self.handle.abort();
            return Some(Session::new(
                self.id,
                self.remote_addr,
                self.sender.clone(),
                self.transport,
            ));
        }
        None
    }
}

pub struct Session {
    id: SessionId,
    pub remote_addr: SocketAddr,
    sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    messages_sender: Sender<Vec<u8>>,
    messages_receiver: Receiver<Vec<u8>>,
    wakers: Arc<Mutex<BTreeMap<Uuid, Waker>>>,
    heartbeat: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
    transport: Arc<tokio::sync::Mutex<LossyTransportState>>,
}

impl Session {
    fn new(
        id: SessionId,
        remote_addr: SocketAddr,
        sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
        transport: LossyTransportState,
    ) -> Session {
        let (messages_sender, messages_receiver) = unbounded();
        let wakers = Arc::new(Mutex::new(BTreeMap::new()));
        let transport = Arc::new(tokio::sync::Mutex::new(transport));

        let heartbeat = Arc::new(AtomicBool::new(true));
        let dead = Arc::new(AtomicBool::new(false));
        let dead_clone = dead.clone();

        let sender_clone = sender.clone();
        let transport_clone = transport.clone();
        let keepalive_serialized = bincode::serialize(&Message::Keepalive).unwrap();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                let mut encrypted = [0u8; 65535];
                let mut guard = transport_clone.lock().await;
                let len = guard
                    .write_message(&keepalive_serialized, &mut encrypted)
                    .await
                    .unwrap();
                let serialized_message =
                    bincode::serialize(&RawMessage::Encrypted(encrypted[..len].to_vec())).unwrap();
                sender_clone
                    .send((
                        remote_addr,
                        SessionPacket::new(id, serialized_message.clone()),
                        64,
                    ))
                    .unwrap();
                if dead_clone.load(Ordering::Acquire) {
                    break;
                }
            }
        });

        let heartbeat_clone = heartbeat.clone();
        let dead_clone = dead.clone();
        let wakers_clone = wakers.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(5000));
            loop {
                interval.tick().await;
                if !heartbeat_clone.load(Ordering::Acquire) {
                    // This session has died, so clean up all remaining wakers
                    dead_clone.store(true, Ordering::Release);
                    let guard = wakers_clone.lock().unwrap();
                    guard
                        .clone()
                        .values()
                        .into_iter()
                        .for_each(|w: &Waker| w.wake_by_ref());
                    break;
                }
                heartbeat_clone.store(false, Ordering::Release);
            }
        });
        Session {
            id,
            remote_addr,
            sender,
            messages_sender,
            messages_receiver,
            wakers,
            heartbeat,
            dead,
            transport,
        }
    }
    pub async fn handle_incoming(&mut self, packet: SessionPacket) {
        match bincode::deserialize::<RawMessage>(&packet.data).unwrap() {
            RawMessage::HandshakePoke => {}
            RawMessage::HandshakeInitiation(_) => {}
            RawMessage::HandshakeResponse(_) => {}
            RawMessage::Encrypted(encrypted) => {
                let mut data = [0u8; 65535];
                let mut guard = self.transport.lock().await;
                if let Ok(len) = guard.read_message(&encrypted, &mut data).await {
                    if let Ok(message) = bincode::deserialize::<Message>(&data[..len]) {
                        match message {
                            Message::Keepalive => {
                                self.heartbeat.store(true, Ordering::Release);
                            }
                            Message::Payload(data) => {
                                self.messages_sender.send(data).unwrap();
                                let mut wakers_guard = self.wakers.lock().unwrap();
                                if let Some((_uuid, w)) = wakers_guard.pop_first() {
                                    w.wake_by_ref()
                                }
                            }
                        }
                    }
                }
            }
        };
    }
    pub async fn send(&mut self, data: Vec<u8>) -> bool {
        if self.is_dead() {
            return false;
        }
        let serialized_message = bincode::serialize(&Message::Payload(data)).unwrap();
        let mut encrypted = [0u8; 65535];
        let mut guard = self.transport.lock().await;
        if let Ok(len) = guard
            .write_message(&serialized_message, &mut encrypted)
            .await
        {
            let message = RawMessage::Encrypted(encrypted[..len].to_vec());
            let serialized = bincode::serialize(&message).unwrap();
            let packet = SessionPacket::new(self.id, serialized);
            self.sender.send((self.remote_addr, packet, 64)).unwrap();
        }
        true
    }
    pub fn recv(&mut self) -> Option<Receiving> {
        if self.is_dead() {
            return None;
        }
        Some(Receiving::new(
            self.wakers.clone(),
            self.messages_receiver.clone(),
            self.dead.clone(),
        ))
    }
    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }
    pub fn close(&self) {
        self.dead.store(true, Ordering::Release);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

pub struct Receiving {
    wakers: Arc<Mutex<BTreeMap<Uuid, Waker>>>,
    receiver: Receiver<Vec<u8>>,
    dead: Arc<AtomicBool>,
    id: Uuid,
}

impl Receiving {
    pub fn new(
        wakers: Arc<Mutex<BTreeMap<Uuid, Waker>>>,
        receiver: Receiver<Vec<u8>>,
        dead: Arc<AtomicBool>,
    ) -> Receiving {
        let id = Uuid::new_v4();
        Receiving {
            wakers,
            receiver,
            dead,
            id,
        }
    }
}

impl Future for Receiving {
    type Output = Result<Vec<u8>, VeqError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.dead.load(Ordering::Acquire) {
            return Poll::Ready(Err(VeqError::Disconnected));
        }
        match self.receiver.try_recv() {
            Ok(data) => {
                let mut guard = self.wakers.lock().unwrap();
                guard.remove(&self.id);
                Poll::Ready(Ok(data))
            }
            Err(_) => {
                let mut guard = self.wakers.lock().unwrap();
                guard.insert(self.id, cx.waker().to_owned());
                Poll::Pending
            }
        }
    }
}
