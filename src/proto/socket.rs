use std::{collections::{HashMap, VecDeque}, future::Future, net::SocketAddr, sync::{Arc}, task::{Poll, Waker}};

use async_std::{net::UdpSocket, sync::Mutex};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::transport::{AddressedReceiver, AddressedSender};

use super::handler::UdppHandler;

#[derive(Debug)]
pub struct UdppServer {
    handler: Arc<Mutex<UdppHandler>>,
    processor_handle: JoinHandle<()>,
}

impl UdppServer {
    pub fn new(external_sender: Box<dyn AddressedSender + Send>, mut external_receiver: Box<dyn AddressedReceiver + Send>) -> UdppServer {
        let handler = Arc::new(Mutex::new(UdppHandler::new(external_sender)));
        let handler_clone = handler.clone();
        let processor_handle = tokio::spawn(async move {
            loop {
                println!("foo");
                if let Ok((src, data)) = external_receiver.addressed_recv().await {
                    println!("foo1");
                    let mut handler_guard = handler_clone.lock().await;
                    handler_guard.handle_incoming(src, data).await;
                    println!("foo2");
                }
            }
        });
        UdppServer {
            handler,
            processor_handle,
        }
    }

    pub fn from_socket(socket: Arc<UdpSocket>) -> UdppServer {
        UdppServer::new(Box::new(socket.clone()), Box::new(socket))
    }

    pub fn incoming(self) -> IncomingUdppSessions {
        IncomingUdppSessions {
            handler: self.handler,
            processor_handle: self.processor_handle,
        }
    }
}

#[derive(Debug)]
pub struct IncomingUdppSessions {
    handler: Arc<Mutex<UdppHandler>>,
    pub processor_handle: JoinHandle<()>,
}

impl IncomingUdppSessions {
    pub async fn accept(&mut self) -> UdppSession {
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
    pub async fn remote_address(&self) -> SocketAddr {
        let handler_guard = self.handler.lock().await;
        let session = handler_guard.sessions.get(&self.session_id).unwrap();
        session.remote_address
    }
    pub async fn connect(addr: SocketAddr) -> Result<UdppSession, std::io::Error> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(UdppSession::from_socket(socket, addr).await)
    }
    pub async fn from_socket(socket: UdpSocket, addr: SocketAddr) -> UdppSession {
        let socket_arc = Arc::new(socket);
        UdppSession::new(Box::new(socket_arc.clone()), Box::new(socket_arc), addr).await
    }
    pub async fn send(&mut self, data: Vec<u8>) {
        let mut handler_guard = self.handler.lock().await;
        handler_guard.send_data(self.session_id, data).await;
    }
    pub async fn try_recv(&mut self) -> Option<Vec<u8>> {
        let mut handler_guard = self.handler.lock().await;
        handler_guard.try_recv_session(self.session_id)
    }
    pub async fn recv(&mut self) -> Vec<u8> {
        let wakers = {
            let handler_guard = self.handler.lock().await;
            handler_guard.wakers.clone()
        };
        let payload_buffer = {
            let handler_guard = self.handler.lock().await;
            let session = handler_guard.sessions.get(&self.session_id).unwrap();
            session.payload_buffer.clone()
        };
        (SessionRecv { session_id: self.session_id, wakers, payload_buffer }).await
    }
}

pub struct SessionRecv {
    session_id: Uuid,
    wakers: Arc<std::sync::Mutex<HashMap<Uuid, Waker>>>,
    payload_buffer: Arc<std::sync::Mutex<VecDeque<Vec<u8>>>>,
}

impl Future for SessionRecv {
    type Output = Vec<u8>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        println!("poll sessionrecv");
        let mut payload_buffer_guard = self.payload_buffer.lock().unwrap();
        match payload_buffer_guard.pop_front() {
            Some(data) => Poll::Ready(data),
            None => {
                let mut wakers_guard = self.wakers.lock().unwrap();
                wakers_guard.entry(self.session_id).or_insert_with(|| cx.waker().clone());
                Poll::Pending
            },
        }
    }
}