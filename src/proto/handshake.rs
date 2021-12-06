use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};

pub struct SnowInitiator(HandshakeState, Option<HandshakeInitiation>);
impl SnowInitiator {
    pub fn new(builder: Builder) -> SnowInitiator {
        SnowInitiator(builder.build_initiator().unwrap(), None)
    }
    pub fn initiation(&mut self) -> HandshakeInitiation {
        match &self.1 {
            Some(initiation) => initiation.clone(),
            None => {
                let initiation = HandshakeInitiation::new(self);
                self.1 = Some(initiation.clone());
                initiation
            }
        }
    }
}

pub struct SnowResponder(HandshakeState);
impl SnowResponder {
    pub fn new(builder: Builder) -> SnowResponder {
        SnowResponder(builder.build_responder().unwrap())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HandshakeInitiation(Vec<u8>);

impl HandshakeInitiation {
    pub fn new(initiator: &mut SnowInitiator) -> HandshakeInitiation {
        let mut initiation_buf = [0u8; 1024];
        let len = initiator.0.write_message(&[], &mut initiation_buf).unwrap();
        HandshakeInitiation(initiation_buf[..len].to_vec())
    }

    pub fn respond(self, mut responder: SnowResponder) -> (TransportState, HandshakeResponse) {
        let mut read_buf = [0u8; 1024];
        responder
            .0
            .read_message(&self.0[..], &mut read_buf)
            .unwrap();

        let mut response_buf = [0u8; 1024];
        let len = responder.0.write_message(&[], &mut response_buf).unwrap();
        let response = HandshakeResponse(response_buf[..len].to_vec());

        let transport = responder.0.into_transport_mode().unwrap();
        (transport, response)
    }
}

#[derive(Clone, Serialize, Deserialize)]

pub struct HandshakeResponse(Vec<u8>);

impl HandshakeResponse {
    pub fn to_vec(self) -> Vec<u8> {
        self.0
    }
    pub fn transport(self, mut initiator: SnowInitiator) -> TransportState {
        let mut read_buf = [0u8; 1024];
        initiator
            .0
            .read_message(&self.to_vec(), &mut read_buf)
            .unwrap();
        initiator.0.into_transport_mode().unwrap()
    }
}
