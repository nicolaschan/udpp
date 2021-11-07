use std::{net::SocketAddr, sync::{Arc}};

use async_std::sync::Mutex;
use uuid::Uuid;

use crate::transport::{AddressedReceiver, AddressedSender};

use super::handler::UdppHandler;

#[derive(Debug)]
pub struct UdppServer {
    handler: Arc<Mutex<UdppHandler>>,
}

impl UdppServer {
    pub fn new(external_sender: Box<dyn AddressedSender + Send>, mut external_receiver: Box<dyn AddressedReceiver + Send>) -> UdppServer {
        let handler = Arc::new(Mutex::new(UdppHandler::new(external_sender)));
        let handler_clone = handler.clone();
        tokio::spawn(async move {
            loop {
                println!("foo");
                if let Ok((src, data)) = external_receiver.addressed_recv().await {
                    let mut handler_guard = handler_clone.lock().await;
                    handler_guard.handle_incoming(src, data).await;
                    println!("foo2");
                }
            }
        });
        UdppServer {
            handler,
        }
    }

    pub fn incoming(self) -> IncomingUdppSessions {
        IncomingUdppSessions {
            handler: self.handler,
        }
    }
}

#[derive(Debug)]
pub struct IncomingUdppSessions {
    handler: Arc<Mutex<UdppHandler>>,
}

impl IncomingUdppSessions {
    async fn accept(&mut self) -> UdppSession {
        let handler_guard = self.handler.lock().await;
        let session_id = handler_guard.new_sessions_receiver.recv().await.unwrap();
        UdppSession {
            session_id, handler: self.handler.clone()
        }
    }
}

#[derive(Debug)]
pub struct UdppSession {
    session_id: Uuid,
    handler: Arc<Mutex<UdppHandler>>,
}

impl UdppSession {
    pub async fn new(external_sender: Box<dyn AddressedSender + Send>, mut external_receiver: Box<dyn AddressedReceiver + Send>, addr: SocketAddr) -> UdppSession {
        let handler = Arc::new(Mutex::new(UdppHandler::new(external_sender)));
        let handler_clone = handler.clone();
        tokio::spawn(async move {
            loop {
                println!("bar");
                if let Ok((src, data)) = external_receiver.addressed_recv().await {
                    let mut handler_guard = handler_clone.lock().await;
                    handler_guard.handle_incoming(src, data).await;
                    println!("bar2");
                }
            }
        });
        let session_id_future = {
            let mut handler_guard = handler.lock().await;
            handler_guard.establish_session(addr).await
        };
        let session_id = session_id_future.await.unwrap();
        UdppSession {
            session_id,
            handler,
        }
    }
    pub async fn send(&mut self, data: Vec<u8>) {
        let mut handler_guard = self.handler.lock().await;
        handler_guard.send_data(self.session_id, data).await;
    }
    pub async fn try_recv(&mut self) -> Option<Vec<u8>> {
        let mut handler_guard = self.handler.lock().await;
        handler_guard.recv_session(self.session_id)
    }
}