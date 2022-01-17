use std::{net::SocketAddr, collections::{HashMap, HashSet}, future::Future, sync::Arc, io::Error, ops::{RangeBounds, Add}, task::{Waker, Poll, Context}, pin::Pin};

use tokio::sync::{mpsc::{Receiver, Sender, channel, unbounded_channel, UnboundedSender, UnboundedReceiver}, Mutex};

use crate::{session::{Session, SessionPacket, SessionId, PendingSessionInitiator, PendingSessionResponder, Receiving, PendingSessionPoker}, veq::{ConnectionInfo, VeqError}, snow_types::{SnowPublicKey, SnowKeypair, SnowInitiator, SnowResponder}};

pub struct Handler {
    outgoing_sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    outgoing_receiver: UnboundedReceiver<(SocketAddr, SessionPacket, u32)>,
    pending_sessions_poker: HashMap<SessionId, (PendingSessionPoker, Arc<std::sync::Mutex<Vec<Waker>>>)>,
    pending_sessions_initiator: HashMap<SessionId, (PendingSessionInitiator, Arc<std::sync::Mutex<Vec<Waker>>>)>,
    pending_sessions_responder: HashMap<SessionId, (PendingSessionResponder, Arc<std::sync::Mutex<Vec<Waker>>>)>,
    established_sessions: HashMap<SessionId, Session>,
    alive_session_ids: Arc<std::sync::Mutex<HashSet<SessionId>>>,
    keypair: SnowKeypair,
}

impl Handler {
    pub fn new(keypair: SnowKeypair) -> Handler {
        let (outgoing_sender, outgoing_receiver): (UnboundedSender<(SocketAddr, SessionPacket, u32)>, UnboundedReceiver<(SocketAddr, SessionPacket, u32)>) = unbounded_channel();
        Handler {
            outgoing_sender,
            outgoing_receiver,
            pending_sessions_poker: HashMap::new(),
            pending_sessions_initiator: HashMap::new(),
            pending_sessions_responder: HashMap::new(),
            established_sessions: HashMap::new(),
            alive_session_ids: Arc::new(std::sync::Mutex::new(HashSet::new())),
            keypair,
        }
    }
    pub async fn handle_incoming(&mut self, src: SocketAddr, data: Vec<u8>) {
        let packet: SessionPacket = bincode::deserialize(&data[..]).unwrap();
        let id = packet.id;
        if let Some((mut pending_session, wakers)) = self.pending_sessions_poker.remove(&packet.id) {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.to_responder().await {
                    Some(session) => { self.pending_sessions_responder.insert(id, (session, wakers)); },
                    None => {},
                }
            } else {
                self.pending_sessions_poker.insert(id, (pending_session, wakers));
            }
            return;
        }
        if let Some((mut pending_session, wakers)) = self.pending_sessions_initiator.remove(&packet.id) {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.to_session().await {
                    Some(session) => { self.upgrade_session(id, session, wakers); },
                    None => {},
                }
            } else {
                self.pending_sessions_initiator.insert(id, (pending_session, wakers));
            }
            return;
        }
        if let Some((mut pending_session, wakers)) = self.pending_sessions_responder.remove(&packet.id) {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.to_session().await {
                    Some(session) => { self.upgrade_session(id, session, wakers); },
                    None => {},
                }
            } else {
                self.pending_sessions_responder.insert(id, (pending_session, wakers));
            }
            return;
        }
        if let Some(session) = self.established_sessions.get_mut(&packet.id) {
            session.handle_incoming(packet).await;
        }
    }

    fn upgrade_session(&mut self, id: SessionId, session: Session, wakers: Arc<std::sync::Mutex<Vec<Waker>>>) {
        let mut alive_session_guard = self.alive_session_ids.lock().unwrap();
        alive_session_guard.insert(id);
        self.established_sessions.insert(id, session);
        let waker_guard = wakers.lock().unwrap();
        waker_guard.clone().into_iter().for_each(|w| w.wake_by_ref());
    }

    pub async fn session_ready(&mut self, id: SessionId) -> bool {
        self.established_sessions.contains_key(&id)
    }

    pub async fn send(&mut self, id: SessionId, data: Vec<u8>) -> Result<(), VeqError> {
        if let Some(session) = self.established_sessions.get_mut(&id) {
            if !session.send(data).await {
                return Err(VeqError::Disconnected);
            }
            return Ok(());
        }
        Err(VeqError::Disconnected)
    }
    pub async fn recv_from(&mut self, id: SessionId) -> Option<Receiving> {
        match self.established_sessions.get_mut(&id) {
            Some(session) => Some(session.recv()),
            None => None,
        }
    }

    pub fn remote_addr(&self, id: SessionId) -> Option<SocketAddr> {
        match self.established_sessions.get(&id) {
            Some(session) => Some(session.remote_addr),
            None => None,
        }
    }

    pub async fn try_next_outgoing(&mut self) -> Option<(SocketAddr, Vec<u8>, u32)> {
        let (addr, packet, ttl) = self.outgoing_receiver.try_recv().ok()?;
        let serialized = bincode::serialize(&packet).unwrap();
        Some((addr, serialized, ttl))
    }
    pub async fn initiate(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        let initiator = SnowInitiator::new(&self.keypair, &info.public_key);
        let pending = PendingSessionInitiator::new(self.outgoing_sender.clone(), id, info, initiator).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        self.pending_sessions_initiator.insert(id, (pending, wakers.clone()));
        SessionReady::new(id, wakers, self.alive_session_ids.clone())
    }
    pub async fn respond(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        let responder = SnowResponder::new(&self.keypair, &info.public_key);
        let poking = PendingSessionPoker::new(self.outgoing_sender.clone(), id, info, responder).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        self.pending_sessions_poker.insert(id, (poking, wakers.clone()));
        SessionReady::new(id, wakers, self.alive_session_ids.clone())
    }
}

#[derive(Debug)]
pub struct SessionReady {
    id: SessionId,
    wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
    added_waker: bool,
    alive_session_ids: Arc<std::sync::Mutex<HashSet<SessionId>>>,
}

impl SessionReady {
    pub fn new(id: SessionId, wakers: Arc<std::sync::Mutex<Vec<Waker>>>, alive_session_ids: Arc<std::sync::Mutex<HashSet<SessionId>>>) -> SessionReady {
        SessionReady {
            id, wakers, added_waker: false, alive_session_ids
        }
    }
}

impl Future for SessionReady {
    type Output = SessionId;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let guard = self.alive_session_ids.lock().unwrap();
        if guard.contains(&self.id) {
            return Poll::Ready(self.id);
        }
        drop(guard);
        if !self.added_waker {
            {
                let mut wakers_guard = self.wakers.lock().unwrap();
                wakers_guard.push(cx.waker().to_owned());
            }
            self.added_waker = true;
        }
        Poll::Pending
    }
}