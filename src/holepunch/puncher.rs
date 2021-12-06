use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
    time::Duration,
};

use async_trait::async_trait;
use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use tokio::{task::JoinHandle, time};

use crate::proto::{
    handshake::{HandshakeInitiation, HandshakeResponse},
    session::{SessionId},
};

use super::punching::{self, HolepunchKeepalive, Holepunching};

#[async_trait]
pub trait HolepunchSender {
    async fn send(&self, addr: SocketAddr, packet: HandshakeInitiation);
}

#[async_trait]
impl HolepunchSender for async_std::channel::Sender<(SocketAddr, HandshakeInitiation)> {
    async fn send(&self, addr: SocketAddr, packet: HandshakeInitiation) {
        self.send((addr, packet)).await.unwrap();
    }
}

pub enum HolepunchPacket {
    HandshakeInitiation(HandshakeInitiation),
    HandshakeResponse(HandshakeResponse),
    HolepunchKeepalive(HolepunchKeepalive),
}

pub struct Initiating(Box<dyn HolepunchSender + Send + Sync>);
pub struct Pending(Arc<Mutex<JoinHandle<()>>>);
pub struct Responding;

pub struct Holepuncher<State> {
    addr: SocketAddr,
    session_id: SessionId,
    packet_sender: Sender<HandshakeResponse>,
    packet_receiver: Receiver<HandshakeResponse>,
    waker: Arc<Mutex<Option<Waker>>>,
    state: State,
}

impl Holepuncher<Initiating> {
    pub fn new(
        addr: SocketAddr,
        sender: Box<dyn HolepunchSender + Send + Sync>,
    ) -> Holepuncher<Initiating> {
        let session_id = SessionId::new_v4();
        let (packet_sender, packet_receiver) = unbounded();
        let waker = Arc::new(Mutex::new(None));
        Holepuncher::<Initiating> {
            addr,
            session_id,
            packet_sender,
            packet_receiver,
            waker,
            state: Initiating(sender),
        }
    }
    pub async fn punch(self) -> (PunchingHole, Holepuncher<Pending>) {
        let initiator = Holepunching::<punching::Initiating>::new();
        let initiation = initiator.initiation();
        let packet_receiver_clone = self.packet_receiver.clone();
        let waker_clone = self.waker.clone();
        let puncher = Holepuncher::<Pending>::new(self, initiation).await;
        let hole = PunchingHole::new(
            packet_receiver_clone,
            waker_clone,
            initiator,
            puncher.handle(),
        );
        (hole, puncher)
    }
}

impl Holepuncher<Pending> {
    pub async fn new(
        initiator: Holepuncher<Initiating>,
        initiation: HandshakeInitiation,
    ) -> Holepuncher<Pending> {
        let addr_clone = initiator.addr;
        let sender = initiator.state.0;
        let handle = Arc::new(Mutex::new(tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
                sender
                    .send(
                        addr_clone,
                        initiation.clone(),
                    )
                    .await;
            }
        })));
        Holepuncher::<Pending> {
            addr: initiator.addr,
            session_id: initiator.session_id,
            packet_sender: initiator.packet_sender,
            packet_receiver: initiator.packet_receiver,
            waker: initiator.waker,
            state: Pending(handle),
        }
    }
    pub fn handle(&self) -> Arc<Mutex<JoinHandle<()>>> {
        self.state.0.clone()
    }
    pub fn dispatch(&self, packet: HandshakeResponse) {
        self.packet_sender.send(packet).unwrap();
        let guard = self.waker.lock().unwrap();
        if let Some(waker) = guard.as_ref() {
            waker.wake_by_ref();
        }
    }
}

pub struct PunchingHole {
    packet_receiver: Receiver<HandshakeResponse>,
    waker: Arc<Mutex<Option<Waker>>>,
    initiator: Option<Holepunching<punching::Initiating>>,
    handle: Arc<Mutex<JoinHandle<()>>>,
}

impl PunchingHole {
    pub fn new(
        packet_receiver: Receiver<HandshakeResponse>,
        waker: Arc<Mutex<Option<Waker>>>,
        initiator: Holepunching<punching::Initiating>,
        handle: Arc<Mutex<JoinHandle<()>>>,
    ) -> PunchingHole {
        PunchingHole {
            packet_receiver,
            waker,
            initiator: Some(initiator),
            handle,
        }
    }
}

impl Future for PunchingHole {
    type Output = Option<Hole>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.packet_receiver.try_recv() {
            Ok(response) => {
                self.handle.lock().unwrap().abort();
                Poll::Ready(Some(Hole::new(
                    self.initiator.take().unwrap().receive_response(response),
                )))
            }
            Err(TryRecvError::Disconnected) => {
                self.handle.lock().unwrap().abort();
                Poll::Ready(None)
            }
            Err(TryRecvError::Empty) => {
                let mut waker = self.waker.lock().unwrap();
                if waker.is_none() {
                    waker.get_or_insert(cx.waker().to_owned());
                }
                Poll::Pending
            }
        }
    }
}

pub struct Hole {
    punched: Holepunching<punching::Punched>,
    handle: JoinHandle<()>,
}

impl Hole {
    pub fn new(punched: Holepunching<punching::Punched>) -> Hole {
        let handle = tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(1000));
            loop {
                interval.tick().await;
            }
        });
        Hole { punched, handle }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{SocketAddr, ToSocketAddrs};

    use async_std::channel::{unbounded, Receiver, Sender};

    use crate::proto::handshake::HandshakeInitiation;

    use super::{HolepunchPacket, Holepuncher, Initiating};

    fn addr() -> SocketAddr {
        "127.0.0.1:8080"
            .to_socket_addrs()
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    #[tokio::test]
    async fn test_initiator_sends_packet() {
        let addr = addr();
        let (sender, receiver): (
            Sender<(SocketAddr, HandshakeInitiation)>,
            Receiver<(SocketAddr, HandshakeInitiation)>,
        ) = unbounded();
        let initiator = Holepuncher::<Initiating>::new(addr, Box::new(sender));
        let (_hole_future, _punch) = initiator.punch().await;
        let (recv_addr, _packet) = receiver.recv().await.unwrap();
        assert_eq!(addr, recv_addr);
    }

    #[tokio::test]
    async fn test_punch() {
        let addr = addr();


    }
}
