use std::{hash::Hash, collections::HashSet, sync::Arc};
use std::io::{Read, Write};

use serde::{Serialize, Deserialize};
use snow::{Builder, Keypair, HandshakeState, TransportState, StatelessTransportState};
use tokio::sync::Mutex;

static NOISE_PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
pub fn builder<'a>() -> Builder<'a> {
    snow::Builder::new(NOISE_PARAMS.parse().unwrap())
}

pub struct LossyTransportState {
    stateless: StatelessTransportState,
    next_nonce: Arc<Mutex<u64>>,
    received_nonces: Arc<Mutex<HashSet<u64>>>,
    size: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Nonced(u64, Vec<u8>);

impl LossyTransportState {
    pub fn new(stateless: StatelessTransportState) -> LossyTransportState {
        LossyTransportState {
            stateless, 
            next_nonce: Arc::new(Mutex::new(0u64)),
            received_nonces: Arc::new(Mutex::new(HashSet::new())), 
            size: 1_000_000_000
        }
    }
    pub async fn write_message(&mut self, payload: &[u8], message: &mut [u8]) -> Result<usize, snow::Error> {
        let mut guard = self.next_nonce.lock().await;
        let nonce  = guard.clone();
        (*guard) = *guard + 1;

        let len= self.stateless.write_message(nonce, payload, message)?;

        let nonced = Nonced(nonce, message[..len].to_vec());
        let mut serialized = bincode::serialize(&nonced).unwrap();

        let len = std::io::copy(&mut serialized[..], message).unwrap();
        Ok(len as usize)
    }
    pub async fn read_message(&mut self, nonce: u64, payload: &[u8], message: &mut [u8]) -> Result<usize, snow::Error> {
        self.stateless.read_message(nonce, payload, message)
    }
}

pub struct SnowKeypair(Keypair);
#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Eq, PartialEq, Hash)]
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

#[derive(Clone, Serialize, Deserialize)]
pub struct SnowInitiation(Vec<u8>);

pub struct SnowInitiator(HandshakeState, SnowInitiation);

impl SnowInitiator {
    pub fn new(keypair: &SnowKeypair, remote_public_key: &SnowPublicKey) -> SnowInitiator {
        let mut state = keypair.builder()
            .remote_public_key(&remote_public_key.0[..])
            .build_initiator()
            .unwrap();
        let mut buf = [0u8; 65535];
        let len = state.write_message(&[], &mut buf).unwrap();
        let message = SnowInitiation(buf[..len].to_vec());
        SnowInitiator(state, message)
    }
    pub fn initiation(&self) -> SnowInitiation {
        self.1.clone()
    }
    pub fn receive_response(mut self, response: SnowResponse) -> LossyTransportState {
        let mut buf = [0u8; 65535];
        self.0.read_message(&response.0, &mut buf).unwrap();
        LossyTransportState::new(self.0.into_stateless_transport_mode().unwrap())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnowResponse(Vec<u8>);

pub struct SnowResponder(HandshakeState);

impl SnowResponder {
    pub fn new(keypair: &SnowKeypair, remote_public_key: &SnowPublicKey) -> SnowResponder {
        let state = keypair.builder()
            .remote_public_key(&remote_public_key.0[..])
            .build_responder()
            .unwrap();
        SnowResponder(state)
    }
    pub fn response(mut self, initiation: SnowInitiation) -> (LossyTransportState, SnowResponse) {
        let mut temp = [0u8; 65535];
        self.0.read_message(&initiation.0, &mut temp).unwrap();
        let mut buf = [0u8; 65535];
        let len = self.0.write_message(&[], &mut buf).unwrap();
        let message = SnowResponse(buf[..len].to_vec());
        (LossyTransportState::new(self.0.into_stateless_transport_mode().unwrap()), SnowResponse(buf[..len].to_vec()))
    }
}


#[cfg(test)]
mod tests {
    use super::{SnowKeypair, SnowInitiator, SnowResponder};

    #[tokio::test]
    async fn test_initiator_to_responder() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation);
        let mut transport1 = initiator.receive_response(response);

        let data = vec![1,2,3,4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport2.read_message(0, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport2.read_message(1, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[tokio::test]
    async fn test_responder_to_initiator() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation);
        let mut transport1 = initiator.receive_response(response);

        let data = vec![1,2,3,4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport1.read_message(0, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport1.read_message(1, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[tokio::test]
    async fn test_bidirectional() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation);
        let mut transport1 = initiator.receive_response(response);

        let data = vec![1,2,3,4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport2.read_message(0, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2.write_message(&data, &mut encrypted_buf).await.unwrap();
        let len = transport1.read_message(1, &encrypted_buf[..len], &mut result_buf).await.unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }
}