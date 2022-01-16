use std::{
    collections::VecDeque,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use crossbeam::channel::{unbounded, Receiver, Sender, TryRecvError};
use snow::{types::Cipher, Builder, Keypair, TransportState};
use uuid::Uuid;

use super::{
    data::{EncryptedMessage, Message, SnowKeypair},
    handshake::{HandshakeInitiation, HandshakeResponse, SnowInitiator, SnowResponder},
};

pub type SessionId = Uuid;

#[derive(PartialEq, Eq)]
pub enum SessionState {
    Initiator,
    Responder,
    Connected,
    Closed,
}

pub struct SessionInitiator<State> {
    keypair: SnowKeypair,
    state: State,
}

impl<T> SessionInitiator<T> {}

impl SessionInitiator<SnowInitiator> {
    pub fn new() -> SessionInitiator<SnowInitiator> {
        let keypair = SnowKeypair::new();
        SessionInitiator::<SnowInitiator>::from_keypair(keypair)
    }
    pub fn from_keypair(keypair: SnowKeypair) -> SessionInitiator<SnowInitiator> {
        let state = SnowInitiator::new(keypair.builder());
        SessionInitiator::<SnowInitiator> { keypair, state }
    }
    pub fn initiation(&mut self) -> HandshakeInitiation {
        self.state.initiation()
    }

    pub fn receive_response(self, response: HandshakeResponse) -> Session {
        Session::new(response.transport(self.state))
    }
}

impl SessionInitiator<SnowResponder> {
    pub fn new() -> SessionInitiator<SnowResponder> {
        let keypair = SnowKeypair::new();
        SessionInitiator::<SnowResponder>::from_keypair(keypair)
    }
    pub fn from_keypair(keypair: SnowKeypair) -> SessionInitiator<SnowResponder> {
        let state = SnowResponder::new(keypair.builder());
        SessionInitiator::<SnowResponder> { keypair, state }
    }
    pub fn receive_initiation(
        self,
        initiation: HandshakeInitiation,
    ) -> (Session, HandshakeResponse) {
        let (transport, response) = initiation.respond(self.state);
        (Session::new(transport), response)
    }
}

#[derive(Clone)]
pub struct Session {
    transport: Arc<Mutex<TransportState>>,
    message_sender: Sender<Message>,
    message_receiver: Receiver<Message>,
    waker_sender: Sender<Waker>,
    waker_receiver: Receiver<Waker>,
}

impl Session {
    pub fn new(transport: TransportState) -> Session {
        let (message_sender, message_receiver) = unbounded();
        let (waker_sender, waker_receiver) = unbounded();
        Session {
            transport: Arc::new(Mutex::new(transport)),
            message_sender,
            message_receiver,
            waker_sender,
            waker_receiver,
        }
    }

    pub fn send(&mut self, message: Message) -> EncryptedMessage {
        let mut transport_guard = self.transport.lock().unwrap();
        EncryptedMessage::encrypt(&mut transport_guard, message)
    }

    pub fn handle_incoming(&mut self, encrypted_message: &EncryptedMessage) {
        let message = {
            let mut transport_guard = self.transport.lock().unwrap();
            encrypted_message.decrypt(&mut transport_guard)
        };
        self.message_sender.send(message.unwrap()).unwrap();
        self.notify_next_waker();
    }

    fn notify_next_waker(&mut self) {
        if let Ok(waker) = self.waker_receiver.try_recv() {
            waker.wake_by_ref();
        }
    }

    pub fn try_recv(&mut self) -> Option<Message> {
        self.message_receiver.try_recv().ok()
    }

    pub fn recv(&mut self) -> MessageFuture {
        MessageFuture::new(self.message_receiver.clone(), self.waker_sender.clone())
    }
}

pub struct MessageFuture {
    registered: bool,
    message_receiver: Receiver<Message>,
    waker_sender: Sender<Waker>,
}

impl MessageFuture {
    pub fn new(message_receiver: Receiver<Message>, waker_sender: Sender<Waker>) -> MessageFuture {
        MessageFuture {
            registered: false,
            message_receiver,
            waker_sender,
        }
    }
}

impl Future for MessageFuture {
    type Output = Option<Message>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.message_receiver.try_recv() {
            Ok(message) => Poll::Ready(Some(message)),
            Err(TryRecvError::Empty) => {
                if !self.registered {
                    if self.waker_sender.send(cx.waker().to_owned()).is_err() {
                        return Poll::Ready(None);
                    }
                    self.registered = true;
                }
                Poll::Pending
            }
            Err(TryRecvError::Disconnected) => Poll::Ready(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::proto::{
        data::Message,
        handshake::{SnowInitiator, SnowResponder},
    };

    use super::{Session, SessionInitiator};

    fn get_sessions() -> (Session, Session) {
        let mut initiator = SessionInitiator::<SnowInitiator>::new();
        let responder = SessionInitiator::<SnowResponder>::new();

        let initiation = initiator.initiation();
        let (session2, response) = responder.receive_initiation(initiation);

        let session1 = initiator.receive_response(response);
        (session1, session2)
    }

    #[test]
    fn test_handshake() {
        get_sessions();
    }

    #[test]
    fn test_multiple_initiations() {
        let mut initiator = SessionInitiator::<SnowInitiator>::new();
        let responder = SessionInitiator::<SnowResponder>::new();

        initiator.initiation();
        initiator.initiation();
        let initiation = initiator.initiation();
        let (_session2, response) = responder.receive_initiation(initiation);

        let _session1 = initiator.receive_response(response);
    }

    #[test]
    fn test_send_recv() {
        let (mut session1, mut session2) = get_sessions();
        let message = Message::new(vec![1, 2, 3, 4]);

        session2.handle_incoming(&session1.send(message.clone()));
        let received = session2.try_recv().unwrap();
        assert_eq!(message, received);
    }

    #[test]
    fn test_bidirectional() {
        let (mut session1, mut session2) = get_sessions();
        let message1 = Message::new(vec![1, 2, 3, 4]);
        let message2 = Message::new(vec![5, 6, 7, 8]);

        session2.handle_incoming(&session1.send(message1.clone()));
        session1.handle_incoming(&session2.send(message2.clone()));

        assert_eq!(message1, session2.try_recv().unwrap());
        assert_eq!(message2, session1.try_recv().unwrap());
    }

    #[test]
    fn test_recv_in_order() {
        let (mut session1, mut session2) = get_sessions();
        let message1 = Message::new((1..10).collect());
        let message2 = Message::new((10..20).collect());
        let message3 = Message::new((20..30).collect());

        session2.handle_incoming(&session1.send(message1.clone()));
        session2.handle_incoming(&session1.send(message2.clone()));
        session2.handle_incoming(&session1.send(message3.clone()));

        assert_eq!(message1, session2.try_recv().unwrap());
        assert_eq!(message2, session2.try_recv().unwrap());
        assert_eq!(message3, session2.try_recv().unwrap());
    }

    #[tokio::test]
    async fn test_async_recv() {
        let (mut session1, mut session2) = get_sessions();
        let message1 = Message::new((1..10).collect());

        let message1_clone = message1.clone();
        let mut session2_clone = session2.clone();

        let handle = tokio::spawn(async move {
            let received = session2_clone.recv().await.unwrap();
            assert_eq!(message1_clone, received);
        });
        session2.handle_incoming(&session1.send(message1.clone()));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_async_recv_in_order() {
        let (mut session1, mut session2) = get_sessions();
        let message1 = Message::new((1..10).collect());
        let message2 = Message::new((10..20).collect());
        let message3 = Message::new((20..30).collect());

        session2.handle_incoming(&session1.send(message1.clone()));
        session2.handle_incoming(&session1.send(message2.clone()));
        session2.handle_incoming(&session1.send(message3.clone()));

        assert_eq!(message1, session2.recv().await.unwrap());
        assert_eq!(message2, session2.recv().await.unwrap());
        assert_eq!(message3, session2.recv().await.unwrap());
    }

    #[test]
    #[should_panic]
    fn test_replay() {
        let (mut session1, mut session2) = get_sessions();
        let message = Message::new(vec![1, 2, 3, 4]);

        let encrypted = session1.send(message.clone());
        session2.handle_incoming(&encrypted);
        session2.handle_incoming(&encrypted);
        let received = session2.try_recv().unwrap();
        assert_eq!(message, received);
    }
}
