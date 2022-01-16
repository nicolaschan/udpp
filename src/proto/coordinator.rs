use std::{sync::Arc, future::Future, task::{Context, Poll}, pin::Pin};
use async_trait::async_trait;

use tokio::{sync::Mutex, task::JoinHandle};

use crate::{transport::{AddressedSender, AddressedReceiver}, ConnectionInfo};

use super::{handler::Handler, bidirectional::Bidirectional};

pub struct Dispatcher {
    handle: JoinHandle<()>,
}

impl Dispatcher {
    pub fn new<T: AddressedReceiver + Send + Sync + 'static>(receiver: T, handler: Arc<Mutex<Handler>>) -> Dispatcher {
        let handle = tokio::spawn(async move {
            loop {
                let (src, packet) = receiver.recv().await;
                let session_packet = bincode::deserialize(&packet[..]).unwrap();
                let mut guard = handler.lock().await;
                guard.dispatch(src, session_packet).await;
            }
        });
        Dispatcher { handle }
    }
}

pub struct PendingConnection {
    peer_info: ConnectionInfo,
    handler: Arc<Mutex<Handler>>,
}
pub struct Connection;

impl Future for PendingConnection {
    type Output = Connection;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Connection)
    }
}

#[async_trait]
impl Bidirectional for Connection {
    async fn send(&mut self, data: Vec<u8>) {
        todo!()
    }

    async fn recv(&mut self) -> Vec<u8> {
        todo!()
    }
}

pub struct Coordinator {
    dispatcher: Dispatcher,
    handler: Arc<Mutex<Handler>>
}

impl Coordinator {
    pub fn new<S: AddressedSender, R: AddressedReceiver + Send + Sync + 'static>(sender: S, receiver: R) -> Coordinator {
        let handler = Arc::new(Mutex::new(Handler::new()));
        let dispatcher = Dispatcher::new(receiver, handler.clone());
        Coordinator { 
            dispatcher,
            handler,
        }
    }

    pub fn connect(&mut self, peer_info: ConnectionInfo) -> PendingConnection {
        PendingConnection {
            peer_info,
            handler: self.handler.clone(),
        }
    }
}