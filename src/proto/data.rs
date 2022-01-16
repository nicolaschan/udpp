use serde::{Deserialize, Serialize};
use snow::{TransportState, Keypair, Builder};

use super::{
    handshake::{HandshakeInitiation, HandshakeResponse},
    session::SessionId,
};

#[derive(Serialize, Deserialize)]
pub struct SessionPacket {
    session_id: SessionId,
    payload: Payload,
}

impl SessionPacket {
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }
    pub fn payload(&self) -> &Payload {
        &self.payload
    }
}

static NOISE_PARAMS: &str = "Noise_IX_25519_ChaChaPoly_BLAKE2s";
pub fn builder<'a>() -> Builder<'a> {
    snow::Builder::new(NOISE_PARAMS.parse().unwrap())
}

pub struct SnowKeypair(Keypair);
#[derive(Clone, Debug)]
pub struct SnowPublicKey(Vec<u8>);
#[derive(Clone, Debug)]
pub struct SnowPrivateKey(Vec<u8>);

impl SnowKeypair {
    pub fn new() -> SnowKeypair {
        let keypair = builder().generate_keypair().unwrap();
        SnowKeypair(keypair)
    }
    pub fn public(&self) -> SnowPublicKey {
        SnowPublicKey(self.0.public.clone())
    }
    pub fn private(&self) -> SnowPrivateKey {
        SnowPrivateKey(self.0.private.clone())
    }
    pub fn builder(&self) -> Builder<'_> {
        builder().local_private_key(&self.0.private)
    }
}

#[derive(Serialize, Deserialize)]
pub struct EncryptedMessage(Vec<u8>);

impl EncryptedMessage {
    pub fn encrypt(transport: &mut TransportState, message: Message) -> EncryptedMessage {
        let mut encrypted_payload = [0u8; 65535];
        let len = transport
            .write_message(&message.0[..], &mut encrypted_payload)
            .unwrap();
        EncryptedMessage(encrypted_payload[..len].to_vec())
    }
    pub fn decrypt(&self, transport: &mut TransportState) -> Result<Message, snow::Error> {
        let mut read_buf = [0u8; 65535];
        let len = transport.read_message(&self.0, &mut read_buf)?;
        Ok(Message(read_buf[..len].to_vec()))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message(Vec<u8>);

impl Message {
    pub fn new(data: Vec<u8>) -> Message {
        Message(data)
    }
}

#[derive(Serialize, Deserialize)]
pub enum Payload {
    HandshakeInitiation(HandshakeInitiation),
    HandshakeResponse(HandshakeResponse),
    EncryptedMessage(EncryptedMessage),
}
