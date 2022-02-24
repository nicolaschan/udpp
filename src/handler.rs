use std::{
    collections::{HashMap, HashSet},
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex,
};

use crate::{
    session::{
        PendingSessionInitiator, PendingSessionPoker, PendingSessionResponder, Receiving, Session,
        SessionId, SessionPacket,
    },
    snow_types::{SnowInitiator, SnowKeypair, SnowResponder},
    veq::{ConnectionInfo, VeqError},
};

type PendingSessionPokerWakers = (PendingSessionPoker, Arc<std::sync::Mutex<Vec<Waker>>>);
type PendingSessionInitiatorWakers = (PendingSessionInitiator, Arc<std::sync::Mutex<Vec<Waker>>>);
type PendingSessionResponderWakers = (PendingSessionResponder, Arc<std::sync::Mutex<Vec<Waker>>>);

#[derive(Clone)]
pub struct Handler {
    outgoing_sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    pending_sessions_poker: Arc<Mutex<HashMap<SessionId, PendingSessionPokerWakers>>>,
    pending_sessions_initiator: Arc<Mutex<HashMap<SessionId, PendingSessionInitiatorWakers>>>,
    pending_sessions_responder: Arc<Mutex<HashMap<SessionId, PendingSessionResponderWakers>>>,
    established_sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
    alive_session_ids: Arc<std::sync::Mutex<HashSet<SessionId>>>,
    keypair: Arc<SnowKeypair>,
}

type PacketSender = UnboundedSender<(SocketAddr, SessionPacket, u32)>;
type PacketReceiver = UnboundedReceiver<(SocketAddr, SessionPacket, u32)>;

impl Handler {
    pub fn new(
        keypair: SnowKeypair,
    ) -> (Handler, UnboundedReceiver<(SocketAddr, SessionPacket, u32)>) {
        let (outgoing_sender, outgoing_receiver): (PacketSender, PacketReceiver) =
            unbounded_channel();
        (
            Handler {
                outgoing_sender,
                pending_sessions_poker: Arc::new(Mutex::new(HashMap::new())),
                pending_sessions_initiator: Arc::new(Mutex::new(HashMap::new())),
                pending_sessions_responder: Arc::new(Mutex::new(HashMap::new())),
                established_sessions: Arc::new(Mutex::new(HashMap::new())),
                alive_session_ids: Arc::new(std::sync::Mutex::new(HashSet::new())),
                keypair: Arc::new(keypair),
            },
            outgoing_receiver,
        )
    }
    pub async fn handle_incoming(&mut self, src: SocketAddr, data: Vec<u8>) {
        let packet: SessionPacket = bincode::deserialize(&data[..]).unwrap();
        let id = packet.id;
        if let Some((mut pending_session, wakers)) = {
            let mut guard = self.pending_sessions_poker.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.responder().await {
                    Some(session) => {
                        let mut guard = self.pending_sessions_responder.lock().await;
                        guard.insert(id, (session, wakers));
                    }
                    None => {}
                }
            } else {
                let mut guard = self.pending_sessions_poker.lock().await;
                guard.insert(id, (pending_session, wakers));
            }
            return;
        }
        if let Some((mut pending_session, wakers)) = {
            let mut guard = self.pending_sessions_initiator.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.session().await {
                    Some(session) => {
                        self.upgrade_session(id, session, wakers).await;
                    }
                    None => {}
                }
            } else {
                let mut guard = self.pending_sessions_initiator.lock().await;
                guard.insert(id, (pending_session, wakers));
            }
            return;
        }
        if let Some((mut pending_session, wakers)) = {
            let mut guard = self.pending_sessions_responder.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.session().await {
                    Some(session) => {
                        self.upgrade_session(id, session, wakers).await;
                    }
                    None => {}
                }
            } else {
                let mut guard = self.pending_sessions_responder.lock().await;
                guard.insert(id, (pending_session, wakers));
            }
            return;
        }
        let mut established_sessions = self.established_sessions.lock().await;
        if let Some(session) = established_sessions.get_mut(&packet.id) {
            session.handle_incoming(packet).await;
        }
    }

    async fn upgrade_session(
        &self,
        id: SessionId,
        session: Session,
        wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
    ) {
        {
            let mut alive_session_guard = self.alive_session_ids.lock().unwrap();
            alive_session_guard.insert(id);
        }
        {
            let mut established_sessions = self.established_sessions.lock().await;
            established_sessions.insert(id, session);
        }
        let waker_guard = wakers.lock().unwrap();
        waker_guard
            .clone()
            .into_iter()
            .for_each(|w| w.wake_by_ref());
    }

    pub async fn send(&mut self, id: SessionId, data: Vec<u8>) -> Result<(), VeqError> {
        let mut established_sessions = self.established_sessions.lock().await;
        if let Some(session) = established_sessions.get_mut(&id) {
            if !session.send(data).await {
                return Err(VeqError::Disconnected);
            }
            return Ok(());
        }
        Err(VeqError::Disconnected)
    }

    pub async fn recv_from(&mut self, id: SessionId) -> Option<Receiving> {
        let mut established_sessions = self.established_sessions.lock().await;
        match established_sessions.get_mut(&id) {
            Some(session) => {
                if session.is_dead() {
                    established_sessions.remove(&id);
                    return None;
                }
                session.recv()
            },
            None => None,
        }
    }

    pub async fn remote_addr(&self, id: SessionId) -> Option<SocketAddr> {
        let established_sessions = self.established_sessions.lock().await;
        established_sessions
            .get(&id)
            .map(|session| session.remote_addr)
    }

    pub async fn initiate(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        self.remove_session(id).await;
        let initiator = SnowInitiator::new(&self.keypair, &info.public_key);
        let pending =
            PendingSessionInitiator::new(self.outgoing_sender.clone(), id, info, initiator).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        let mut guard = self.pending_sessions_initiator.lock().await;
        guard.insert(id, (pending, wakers.clone()));
        SessionReady::new(id, wakers, self.alive_session_ids.clone())
    }
    pub async fn respond(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        self.remove_session(id).await;
        let responder = SnowResponder::new(&self.keypair, &info.public_key);
        let poking =
            PendingSessionPoker::new(self.outgoing_sender.clone(), id, info, responder).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        let mut guard = self.pending_sessions_poker.lock().await;
        guard.insert(id, (poking, wakers.clone()));
        SessionReady::new(id, wakers, self.alive_session_ids.clone())
    }

    async fn remove_session(&mut self, id: SessionId) {
        {
            let mut pokers = self.pending_sessions_poker.lock().await;
            pokers.remove(&id);
        }
        {
            let mut initiating = self.pending_sessions_initiator.lock().await;
            initiating.remove(&id);
        }
        {
            let mut responding = self.pending_sessions_responder.lock().await;
            responding.remove(&id);
        }
        {
            let mut established = self.established_sessions.lock().await;
            established.remove(&id);
        }
        {
            let mut alive_session_guard = self.alive_session_ids.lock().unwrap();
            alive_session_guard.remove(&id);
        }
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
    pub fn new(
        id: SessionId,
        wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
        alive_session_ids: Arc<std::sync::Mutex<HashSet<SessionId>>>,
    ) -> SessionReady {
        SessionReady {
            id,
            wakers,
            added_waker: false,
            alive_session_ids,
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
