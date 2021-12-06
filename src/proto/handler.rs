use std::{collections::HashMap, net::SocketAddr};

use super::{
    data::SessionPacket,
    session::{Session, SessionId},
};

use crate::proto::data::Payload::EncryptedMessage;

pub struct Handler {
    active_sessions: HashMap<SessionId, Session>,
}

impl Handler {
    pub fn new() -> Handler {
        Handler {
            active_sessions: HashMap::new(),
        }
    }

    pub async fn dispatch(&mut self, _src: SocketAddr, packet: SessionPacket) {
        let session_id = packet.session_id();
        if let Some(session) = self.active_sessions.get_mut(&session_id) {
            if let EncryptedMessage(encrypted_message) = packet.payload() {
                session.handle_incoming(encrypted_message);
            }
        }
    }
}
