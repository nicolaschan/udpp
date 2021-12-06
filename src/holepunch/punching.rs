





use async_trait::async_trait;




use crate::proto::handshake::{
    HandshakeInitiation, HandshakeResponse, SnowInitiator, SnowResponder,
};
use crate::proto::session::{Session, SessionInitiator};


pub struct HolepunchKeepalive;

#[async_trait]
pub trait Callback {
    async fn call(&self);
}

pub struct Initiating(SessionInitiator<SnowInitiator>, HandshakeInitiation);
impl Initiating {
    pub fn new() -> Initiating {
        let mut initiator = SessionInitiator::<SnowInitiator>::new();
        let initiation = initiator.initiation();
        Initiating(initiator, initiation)
    }
    pub fn receive_response(self, response: HandshakeResponse) -> Punched {
        let session = self.0.receive_response(response);
        Punched(session)
    }
    pub fn initiation(&self) -> HandshakeInitiation {
        self.1.clone()
    }
}

pub struct Waiting(SessionInitiator<SnowResponder>);

impl Waiting {
    pub fn new() -> Waiting {
        Waiting(SessionInitiator::<SnowResponder>::new())
    }

    pub fn receive_initiation(self, initiation: HandshakeInitiation) -> Responding {
        let (session, response) = self.0.receive_initiation(initiation);
        Responding(session, response)
    }
}

pub struct Responding(Session, HandshakeResponse);
impl Responding {
    pub fn receive_keepalive(self, _keepalive: HolepunchKeepalive) -> Punched {
        Punched(self.0)
    }
}
pub struct Punched(Session);

pub struct Holepunching<State>(State);

impl<State> Holepunching<State> {
    // pub fn new(state: State) -> Holepunching<State> {
    //     Holepunching { state }
    // }
}

impl Holepunching<Initiating> {
    pub fn new() -> Holepunching<Initiating> {
        Holepunching::<Initiating>(Initiating::new())
    }
    pub fn initiation(&self) -> HandshakeInitiation {
        self.0.initiation()
    }
    pub fn receive_response(self, response: HandshakeResponse) -> Holepunching<Punched> {
        let state = self.0.receive_response(response);
        Holepunching::<Punched>(state)
    }
}

impl Holepunching<Waiting> {
    pub fn new() -> Holepunching<Waiting> {
        Holepunching::<Waiting>(Waiting::new())
    }
    pub fn keepalive(&self) -> HolepunchKeepalive {
        HolepunchKeepalive
    }
    pub fn receive_initiation(self, initiation: HandshakeInitiation) -> Holepunching<Responding> {
        Holepunching::<Responding>(self.0.receive_initiation(initiation))
    }
}

impl Holepunching<Responding> {
    pub fn response(&self) -> HandshakeResponse {
        self.0 .1.clone()
    }
    pub fn receive_keepalive(self, keepalive: HolepunchKeepalive) -> Holepunching<Punched> {
        Holepunching::<Punched>(self.0.receive_keepalive(keepalive))
    }
}

impl Holepunching<Punched> {
    pub fn keepalive(&self) -> HolepunchKeepalive {
        HolepunchKeepalive
    }
    pub fn into_session(self) -> Session {
        self.0 .0
    }
}

#[cfg(test)]
mod tests {
    

    
    

    use crate::proto::{data::Message};

    use super::{Holepunching, Initiating, Waiting};

    #[tokio::test]
    async fn test_punch() {
        let initiator = Holepunching::<Initiating>::new();
        let responder = Holepunching::<Waiting>::new();

        let initiation = initiator.initiation();
        let responder = responder.receive_initiation(initiation);

        let response = responder.response();
        let punched1 = initiator.receive_response(response);

        let keepalive = punched1.keepalive();
        let punched2 = responder.receive_keepalive(keepalive);

        let mut session1 = punched1.into_session();
        let mut session2 = punched2.into_session();

        let message = Message::new((1..10).collect());
        let encrypted = session1.send(message.clone());
        session2.handle_incoming(&encrypted);
        let received = session2.recv().await.unwrap();

        assert_eq!(message, received);
    }
}
