use std::{net::SocketAddr, sync::{Arc, Mutex}};

use uuid::Uuid;

use crate::transport::{AddressedReceiver, AddressedSender};

use super::handler::UdppHandler;

#[derive(Debug)]
pub struct UdppServer {
    handler: Arc<Mutex<UdppHandler>>,
}

impl UdppServer {
    pub fn new(sender: Box<dyn AddressedSender + Send>, mut receiver: Box<dyn AddressedReceiver + Send>) -> UdppServer {
        let handler = Arc::new(Mutex::new(UdppHandler::new(sender)));
        let handler_clone = handler.clone();
        tokio::spawn(async move {
            while let Ok((src, data)) = receiver.addressed_recv() {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.handle_incoming(src, data);
                println!("foo");
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

impl Iterator for IncomingUdppSessions {
    type Item = UdppSession;

    fn next(&mut self) -> Option<Self::Item> {
        let handler_guard = self.handler.lock().unwrap();
        let receiver = handler_guard.session_id_receiver();
        receiver.recv().ok().map(|session_id| UdppSession {
            session_id, 
            handler: self.handler.clone()
        })
    }
}

#[derive(Debug)]
pub struct UdppSession {
    session_id: Uuid,
    handler: Arc<Mutex<UdppHandler>>,
}

impl UdppSession {
    pub async fn new(sender: Box<dyn AddressedSender + Send>, mut receiver: Box<dyn AddressedReceiver + Send>, addr: SocketAddr) -> UdppSession {
        let handler = Arc::new(Mutex::new(UdppHandler::new(sender)));
        let handler_clone = handler.clone();
        tokio::spawn(async move {
            while let Ok((src, data)) = receiver.addressed_recv() {
                let mut handler_guard = handler_clone.lock().unwrap();
                handler_guard.handle_incoming(src, data);
            }
        });
        let handler_clone = handler.clone();
        let session_id_future = {
            let mut handler_guard = handler_clone.lock().unwrap();
            handler_guard.establish_session(addr, handler.clone())
        };
        let session_id = session_id_future.await.unwrap();
        UdppSession {
            session_id,
            handler,
        }
    }
    pub fn send(&mut self, data: Vec<u8>) {
        let mut handler_guard = self.handler.lock().unwrap();
        handler_guard.send_data(self.session_id, data);
    }
    pub fn recv(&mut self) -> Option<Vec<u8>> {
        let mut handler_guard = self.handler.lock().unwrap();
        handler_guard.recv_session(self.session_id)
    }
}