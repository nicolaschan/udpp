use std::{future::Future, pin::Pin, task::{Context, Poll, Waker}, net::SocketAddr, time::Duration, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, collections::VecDeque};

use crossbeam::channel::{Receiver, Sender, unbounded};
use serde::{Deserialize, Serialize};
use tokio::{task::JoinHandle, sync::mpsc::{UnboundedSender, UnboundedReceiver, unbounded_channel}, time};
use uuid::Uuid;

use crate::veq::{ConnectionInfo, VeqError};

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
pub enum Message {
    HandshakeInitiation,
    HandshakeResponse,
    Keepalive,
    Data(Vec<u8>),
}

pub struct PendingSessionInitiator {
    conn_info: ConnectionInfo,
    handle: JoinHandle<()>,
    working_remote_addr: Option<SocketAddr>,
    waker: Arc<Mutex<Option<Waker>>>,
    id: SessionId,
    sender: UnboundedSender<(SocketAddr, SessionPacket)>,
}

impl PendingSessionInitiator {
    pub async fn new(sender: UnboundedSender<(SocketAddr, SessionPacket)>, id: SessionId, conn_info: ConnectionInfo) -> PendingSessionInitiator {
        let addresses = conn_info.addresses.clone();
        let serialized_message = bincode::serialize(&Message::HandshakeInitiation).unwrap();
        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                addresses.clone().into_iter().for_each(|addr| {
                    sender_clone.send((addr, SessionPacket::new(id, serialized_message.clone()))).unwrap();
                });
            }
        });
        PendingSessionInitiator {
            conn_info,
            handle,
            working_remote_addr: None,
            waker: Arc::new(Mutex::new(None)),
            id,
            sender,
        }
    }
    pub async fn handle_incoming(&mut self, src: SocketAddr, packet: &SessionPacket) {
        if let Ok(Message::HandshakeResponse) = bincode::deserialize::<Message>(&packet.data) {
            self.working_remote_addr = Some(src);
            let mut guard = self.waker.lock().unwrap();
            if let Some(waker)  = guard.take() {
                waker.wake_by_ref();
            }
        }
    }
    pub async fn to_session(&mut self) -> Option<Session> {
        if let Some(remote_addr) = self.working_remote_addr {
            self.handle.abort();
            return Some(Session::new(self.id, remote_addr, self.sender.clone()));
        }
        return None;
    }
}

pub struct PendingSessionResponder {
    conn_info: ConnectionInfo,
    handle: JoinHandle<()>,
    working_remote_addr: Option<SocketAddr>,
    waker: Arc<Mutex<Option<Waker>>>,
    id: SessionId,
    sender: UnboundedSender<(SocketAddr, SessionPacket)>,
}

impl PendingSessionResponder {
    pub async fn new(sender: UnboundedSender<(SocketAddr, SessionPacket)>, id: SessionId, conn_info: ConnectionInfo) -> PendingSessionResponder {
        let addresses = conn_info.addresses.clone();
        let serialized_message = bincode::serialize(&Message::HandshakeResponse).unwrap();
        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                addresses.clone().into_iter().for_each(|addr| {
                    sender_clone.send((addr, SessionPacket::new(id, serialized_message.clone()))).unwrap();
                });
            }
        });
        PendingSessionResponder {
            conn_info,
            handle,
            working_remote_addr: None,
            waker: Arc::new(Mutex::new(None)),
            id,
            sender,
        }
    }

    pub async fn handle_incoming(&mut self, src: SocketAddr, packet: &SessionPacket) {
        if let Ok(Message::Keepalive) = bincode::deserialize::<Message>(&packet.data) {
            self.working_remote_addr = Some(src);
            let mut guard = self.waker.lock().unwrap();
            if let Some(waker)  = guard.take() {
                waker.wake_by_ref();
            }
        }
    }

    pub async fn to_session(&mut self) -> Option<Session> {
        if let Some(remote_addr) = self.working_remote_addr {
            self.handle.abort();
            return Some(Session::new(self.id, remote_addr, self.sender.clone()));
        }
        return None;
    }
}

pub struct Session {
    id: SessionId,
    remote_addr: SocketAddr,
    sender: UnboundedSender<(SocketAddr, SessionPacket)>,
    messages_sender: Sender<Vec<u8>>,
    messages_receiver: Receiver<Vec<u8>>,
    wakers: Arc<Mutex<VecDeque<Waker>>>,
    handle: JoinHandle<()>,
    death_handle: JoinHandle<()>,
    heartbeat: Arc<AtomicBool>,
    dead: Arc<AtomicBool>,
}

impl Session {
    fn new(id: SessionId, remote_addr: SocketAddr, sender: UnboundedSender<(SocketAddr, SessionPacket)>) -> Session {
        let (messages_sender, messages_receiver) = unbounded();
        let wakers = Arc::new(Mutex::new(VecDeque::new()));

        let heartbeat = Arc::new(AtomicBool::new(true));
        let dead = Arc::new(AtomicBool::new(false));
        let dead_clone = dead.clone();

        let sender_clone = sender.clone();
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            let serialized_message = bincode::serialize(&Message::Keepalive).unwrap();
            loop {
                interval.tick().await;
                sender_clone.send((remote_addr, SessionPacket::new(id, serialized_message.clone()))).unwrap();
                if dead_clone.load(Ordering::Acquire) {
                    break;
                }
            }
        });

        let heartbeat_clone = heartbeat.clone();
        let dead_clone = dead.clone();
        let wakers_clone = wakers.clone();
        let death_handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(5000));
            loop {
                interval.tick().await;
                if !heartbeat_clone.load(Ordering::Acquire) {
                    dead_clone.store(true, Ordering::Release);
                    let guard = wakers_clone.lock().unwrap();
                    guard.clone().into_iter().for_each(|w: Waker| w.wake_by_ref());
                    break;
                }
                heartbeat_clone.store(false, Ordering::Release);
            }
        });
        Session {
            id, remote_addr, sender, messages_sender, messages_receiver, wakers, handle, death_handle, heartbeat, dead
        }
    }
    pub async fn handle_incoming(&mut self, packet: SessionPacket) {
        self.heartbeat.store(true, Ordering::Release);
        match bincode::deserialize::<Message>(&packet.data).unwrap() {
            Message::HandshakeInitiation => {},
            Message::HandshakeResponse => {},
            Message::Keepalive => {},
            Message::Data(data) => {
                self.messages_sender.send(data).unwrap();
                let mut wakers_guard = self.wakers.lock().unwrap();
                wakers_guard.pop_front().map(|w| w.wake_by_ref());
            },
        };
    }
    pub async fn send(&mut self, data: Vec<u8>) -> bool {
        if self.is_dead() {
            return false;
        }
        let message = Message::Data(data);
        let serialized = bincode::serialize(&message).unwrap();
        let packet = SessionPacket::new(self.id, serialized);
        self.sender.send((self.remote_addr, packet)).unwrap();
        return true;
    }
    pub fn recv(&mut self) -> Receiving {
        Receiving::new(self.wakers.clone(), self.messages_receiver.clone(), self.dead.clone())
    }
    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }
}

pub struct Receiving {
    wakers: Arc<Mutex<VecDeque<Waker>>>,
    receiver: Receiver<Vec<u8>>,
    dead: Arc<AtomicBool>,
    added_waker: bool,
}

impl Receiving {
    pub fn new(wakers: Arc<Mutex<VecDeque<Waker>>>, receiver: Receiver<Vec<u8>>, dead: Arc<AtomicBool>) -> Receiving {
        Receiving { wakers, receiver, dead, added_waker: false }     
    }
}

impl Future for Receiving {
    type Output = Result<Vec<u8>, VeqError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.dead.load(Ordering::Acquire) {
            return Poll::Ready(Err(VeqError::Disconnected));
        }
        match self.receiver.try_recv() {
            Ok(data) => Poll::Ready(Ok(data)),
            Err(_) => {
                if !self.added_waker {
                    let mut guard = self.wakers.lock().unwrap();
                    guard.push_back(cx.waker().to_owned());
                }
                Poll::Pending
            }
        }
    }
}