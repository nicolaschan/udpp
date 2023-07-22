use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use log::debug;
use tokio::sync::{
    broadcast::{self, Receiver},
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex,
};
use uuid::Uuid;

use crate::{
    session::{
        PendingSessionInitiator, PendingSessionPoker, PendingSessionResponder, Receiving, Session,
        SessionId, SessionPacket, SessionResult,
    },
    snow_types::{SnowInitiator, SnowKeypair, SnowResponder},
    veq::{ConnectionInfo, VeqError},
};

pub type OneTimeId = SessionId;

type PendingSessionPokerWakers = (
    PendingSessionPoker,
    OneTimeId,
    broadcast::Sender<()>,
    Arc<std::sync::Mutex<Vec<Waker>>>,
);
type PendingSessionInitiatorWakers = (
    PendingSessionInitiator,
    OneTimeId,
    broadcast::Sender<()>,
    Arc<std::sync::Mutex<Vec<Waker>>>,
);
type PendingSessionResponderWakers = (
    PendingSessionResponder,
    OneTimeId,
    broadcast::Sender<()>,
    Arc<std::sync::Mutex<Vec<Waker>>>,
);

#[derive(Clone)]
pub struct Handler {
    outgoing_sender: UnboundedSender<(SocketAddr, SessionPacket, u32)>,
    pending_sessions_poker: Arc<Mutex<HashMap<SessionId, PendingSessionPokerWakers>>>,
    pending_sessions_initiator: Arc<Mutex<HashMap<SessionId, PendingSessionInitiatorWakers>>>,
    pending_sessions_responder: Arc<Mutex<HashMap<SessionId, PendingSessionResponderWakers>>>,
    established_sessions: Arc<Mutex<HashMap<SessionId, (OneTimeId, Session)>>>,
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
                keypair: Arc::new(keypair),
            },
            outgoing_receiver,
        )
    }
    pub async fn handle_incoming(&mut self, src: SocketAddr, data: Vec<u8>) {
        let packet: SessionPacket = bincode::deserialize(&data[..]).unwrap();
        let id = packet.id;
        if let Some((mut pending_session, one_time_id, sender, wakers)) = {
            let mut guard = self.pending_sessions_poker.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                if let Some(session) = pending_session.responder().await {
                    let mut guard = self.pending_sessions_responder.lock().await;
                    guard.insert(id, (session, one_time_id, sender, wakers));
                }
            } else {
                let mut guard = self.pending_sessions_poker.lock().await;
                guard.insert(id, (pending_session, one_time_id, sender, wakers));
            }
            return;
        }
        if let Some((mut pending_session, one_time_id, sender, wakers)) = {
            let mut guard = self.pending_sessions_initiator.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                match pending_session.session().await {
                    SessionResult::Ok(session) => {
                        self.upgrade_session(id, one_time_id, session, sender, wakers).await;
                    }
                    SessionResult::Err(pending_session) => {
                        let mut guard = self.pending_sessions_initiator.lock().await;
                        guard.insert(id, (pending_session, one_time_id, sender, wakers));
                    }
                }
            } else {
                let mut guard = self.pending_sessions_initiator.lock().await;
                guard.insert(id, (pending_session, one_time_id, sender, wakers));
            }
            return;
        }
        if let Some((mut pending_session, one_time_id, sender, wakers)) = {
            let mut guard = self.pending_sessions_responder.lock().await;
            guard.remove(&packet.id)
        } {
            pending_session.handle_incoming(src, &packet).await;
            if pending_session.is_ready() {
                if let Some(session) = pending_session.session().await {
                    self.upgrade_session(id, one_time_id, session, sender, wakers)
                        .await;
                }
            } else {
                let mut guard = self.pending_sessions_responder.lock().await;
                guard.insert(id, (pending_session, one_time_id, sender, wakers));
            }
            return;
        }
        let mut established_sessions = self.established_sessions.lock().await;
        if let Some((_, session)) = established_sessions.get_mut(&packet.id) {
            session.handle_incoming(packet).await;
        }
    }

    async fn upgrade_session(
        &self,
        id: SessionId,
        one_time_id: OneTimeId,
        session: Session,
        sender: broadcast::Sender<()>,
        wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
    ) {
        {
            let mut established_sessions = self.established_sessions.lock().await;
            established_sessions.insert(id, (one_time_id, session));
        }
        if let Err(e) = sender.send(()) {
            log::warn!("Failed to send session ready notification: {}", e);
        }
        let waker_guard = wakers.lock().unwrap();
        waker_guard
            .clone()
            .into_iter()
            .for_each(|w| w.wake_by_ref());
    }

    pub async fn send(&self, id: SessionId, data: Vec<u8>) -> Result<(), VeqError> {
        let mut established_sessions = self.established_sessions.lock().await;
        if let Some((_, session)) = established_sessions.get_mut(&id) {
            if !session.send(data).await {
                return Err(VeqError::Disconnected);
            }
            return Ok(());
        }
        Err(VeqError::Disconnected)
    }

    pub async fn recv_from(&self, id: SessionId) -> Option<Receiving> {
        let mut established_sessions = self.established_sessions.lock().await;
        match established_sessions.get_mut(&id) {
            Some((_, session)) => {
                if session.is_dead() {
                    established_sessions.remove(&id);
                    return None;
                }
                session.recv()
            }
            None => None,
        }
    }

    pub async fn remote_addr(&self, id: SessionId) -> Option<SocketAddr> {
        let established_sessions = self.established_sessions.lock().await;
        established_sessions
            .get(&id)
            .map(|(_, session)| session.remote_addr)
    }

    pub async fn initiate(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        self.remove_session(id, None).await;
        let initiator = SnowInitiator::new(&self.keypair, &info.public_key);
        let pending =
            PendingSessionInitiator::new(self.outgoing_sender.clone(), id, info, initiator).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        let mut guard = self.pending_sessions_initiator.lock().await;
        let (sender, receiver) = broadcast::channel(10);

        let one_time_id = Uuid::new_v4();
        guard.insert(id, (pending, one_time_id, sender, wakers.clone()));
        SessionReady::new(id, one_time_id, wakers, receiver, self.clone())
    }
    pub async fn respond(&mut self, id: SessionId, info: ConnectionInfo) -> SessionReady {
        self.remove_session(id, None).await;
        let responder = SnowResponder::new(&self.keypair, &info.public_key);
        let poking =
            PendingSessionPoker::new(self.outgoing_sender.clone(), id, info, responder).await;
        let wakers = Arc::new(std::sync::Mutex::new(vec![]));
        let mut guard = self.pending_sessions_poker.lock().await;
        let (sender, receiver) = broadcast::channel(10);

        let one_time_id = Uuid::new_v4();
        guard.insert(id, (poking, one_time_id, sender, wakers.clone()));
        SessionReady::new(id, one_time_id, wakers, receiver, self.clone())
    }

    pub fn close_session(&self, id: SessionId, one_time_id: OneTimeId) {
        let handler = self.clone();
        tokio::task::spawn(async move {
            handler.remove_session(id, Some(one_time_id)).await;
        });
    }

    async fn remove_session(&self, id: SessionId, one_time_id: Option<OneTimeId>) {
        debug!("Removing session id={:?} one_time_id={:?}", id, one_time_id);
        {
            let mut established_sessions = self.established_sessions.lock().await;
            if let Some(one_time_id) = &one_time_id {
                if let Some((current_one_time_id, _)) = established_sessions.get(&id) {
                    if one_time_id == current_one_time_id {
                        debug!(
                            "Removing established session id={:?} one_time_id={:?}",
                            id, one_time_id
                        );
                        established_sessions.remove(&id);
                    }
                }
            } else {
                debug!(
                    "Removing established session id={:?} one_time_id={:?}",
                    id, one_time_id
                );
                established_sessions.remove(&id);
            }
        }
        {
            let mut responder = self.pending_sessions_responder.lock().await;
            if let Some(one_time_id) = &one_time_id {
                if let Some((_, current_one_time_id, _, _)) = responder.get(&id) {
                    if one_time_id == current_one_time_id {
                        debug!(
                            "Removing responding session id={:?} one_time_id={:?}",
                            id, one_time_id
                        );
                        responder
                            .remove(&id)
                            .map(|(mut r, _id, _sender, _waker)| {
                                r.abort();
                                (r, _id, _sender, _waker)
                            });
                    }
                }
            } else {
                debug!(
                    "Removing responding session id={:?} one_time_id={:?}",
                    id, one_time_id
                );
                responder
                    .remove(&id)
                    .map(|(mut r, _id, _sender, _waker)| {
                        r.abort();
                        (r, _id, _sender, _waker)
                    });
            }
        }
        {
            let mut pokers = self.pending_sessions_poker.lock().await;
            if let Some(one_time_id) = &one_time_id {
                if let Some((_, current_one_time_id, _, _)) = pokers.get(&id) {
                    if one_time_id == current_one_time_id {
                        debug!(
                            "Removing poker session id={:?} one_time_id={:?}",
                            id, one_time_id
                        );
                        pokers
                            .remove(&id)
                            .map(|(mut r, _id, _sender, _waker)| {
                                r.abort();
                                (r, _id, _sender, _waker)
                            });
                    }
                }
            } else {
                debug!(
                    "Removing poker session id={:?} one_time_id={:?}",
                    id, one_time_id
                );
                pokers
                    .remove(&id)
                    .map(|(mut r, _id, _sender, _waker)| {
                        r.abort();
                        (r, _id, _sender, _waker)
                    });
            }
        }
        {
            let mut initiating = self.pending_sessions_initiator.lock().await;
            if let Some(one_time_id) = &one_time_id {
                if let Some((_, current_one_time_id, _, _)) = initiating.get(&id) {
                    if one_time_id == current_one_time_id {
                        debug!(
                            "Removing initiating session id={:?} one_time_id={:?}",
                            id, one_time_id
                        );
                        initiating
                            .remove(&id)
                            .map(|(mut r, _id, _sender, _waker)| {
                                r.abort();
                                (r, _id, _sender, _waker)
                            });
                    }
                }
            } else {
                debug!(
                    "Removing initiating session id={:?} one_time_id={:?}",
                    id, one_time_id
                );
                initiating
                    .remove(&id)
                    .map(|(mut r, _id, _sender, _waker)| {
                        r.abort();
                        (r, _id, _sender, _waker)
                    });
            }
        }
    }
}

pub struct SessionReady {
    id: SessionId,
    one_time_id: OneTimeId,
    wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
    added_waker: bool,
    receiver: Receiver<()>,
    handler: Handler,
    completed: Arc<std::sync::Mutex<bool>>,
}

impl SessionReady {
    pub fn new(
        id: SessionId,
        one_time_id: OneTimeId,
        wakers: Arc<std::sync::Mutex<Vec<Waker>>>,
        receiver: Receiver<()>,
        handler: Handler,
    ) -> SessionReady {
        SessionReady {
            id,
            one_time_id,
            wakers,
            added_waker: false,
            receiver,
            handler,
            completed: Arc::new(std::sync::Mutex::new(false)),
        }
    }
}

impl Future for SessionReady {
    type Output = (SessionId, OneTimeId);

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.receiver.try_recv().is_ok() {
            let mut completed_guard = self.completed.lock().unwrap();
            *completed_guard = true;
            return Poll::Ready((self.id, self.one_time_id));
        }
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

impl Drop for SessionReady {
    fn drop(&mut self) {
        let completed = *self.completed.lock().unwrap();
        if !completed {
            self.handler.close_session(self.id, self.one_time_id);
        }
    }
}