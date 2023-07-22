use std::{collections::BTreeSet, hash::Hash, sync::Arc};

use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, StatelessTransportState};
use tokio::sync::Mutex;

static NOISE_PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
pub fn builder<'a>() -> Builder<'a> {
    snow::Builder::new(NOISE_PARAMS.parse().unwrap())
}

pub struct LossyTransportState {
    stateless: StatelessTransportState,
    size: usize,
    next_nonce: Arc<Mutex<u64>>,
    received_nonces: Arc<Mutex<BTreeSet<u64>>>,
}

#[derive(Serialize, Deserialize)]
pub struct Nonced(u64, Vec<u8>);

impl LossyTransportState {
    pub fn from_stateless(stateless: StatelessTransportState) -> LossyTransportState {
        LossyTransportState::new(stateless, 1)
    }
    pub fn new(stateless: StatelessTransportState, size: usize) -> LossyTransportState {
        LossyTransportState {
            stateless,
            size,
            next_nonce: Arc::new(Mutex::new(0u64)),
            received_nonces: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }
    pub async fn write_message(
        &mut self,
        payload: &[u8],
        message: &mut [u8],
    ) -> Result<usize, snow::Error> {
        let mut guard = self.next_nonce.lock().await;
        let nonce = *guard;
        (*guard) += 1;

        let len = self.stateless.write_message(nonce, payload, message)?;

        let nonced = Nonced(nonce, message[..len].to_vec());
        let serialized = bincode::serialize(&nonced).unwrap();

        for (d, s) in message.iter_mut().zip(serialized.iter()) {
            *d = *s;
        }
        let len = serialized.len();
        Ok(len)
    }
    pub async fn read_message(
        &mut self,
        payload: &[u8],
        message: &mut [u8],
    ) -> Result<usize, snow::Error> {
        let nonced: Nonced = bincode::deserialize(payload).unwrap();
        let nonce = nonced.0;
        let payload = nonced.1;

        let mut guard = self.received_nonces.lock().await;
        if guard.contains(&nonce) || guard.first().map(|f| f > &nonce).unwrap_or(false) {
            return Err(snow::Error::Decrypt);
        }
        guard.insert(nonce);
        while guard.len() > self.size {
            guard.pop_first();
        }
        self.stateless.read_message(nonce, &payload[..], message)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SnowKeypair(SnowPublicKey, SnowPrivateKey);
#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Eq, PartialEq, Hash)]
pub struct SnowPublicKey(Vec<u8>);

#[derive(Clone, Debug, Serialize, Deserialize, PartialOrd, Eq, PartialEq, Hash)]
pub struct SnowPrivateKey(Vec<u8>);

impl SnowKeypair {
    pub fn new() -> SnowKeypair {
        let keypair = builder().generate_keypair().unwrap();
        SnowKeypair(
            SnowPublicKey(keypair.public.clone()),
            SnowPrivateKey(keypair.private),
        )
    }
    pub fn public(&self) -> SnowPublicKey {
        self.0.clone()
    }
    pub fn private(&self) -> SnowPrivateKey {
        self.1.clone()
    }
    pub fn builder(&self) -> Builder<'_> {
        builder().local_private_key(&self.1 .0)
    }
}

impl Default for SnowKeypair {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnowInitiation(Vec<u8>);

#[derive(Debug)]
pub struct SnowInitiator(HandshakeState, SnowInitiation);

pub enum ReceiveResponseResult {
    Ok(LossyTransportState),
    Err(SnowInitiator),
}

impl ReceiveResponseResult {
    pub fn unwrap(self) -> LossyTransportState {
        match self {
            ReceiveResponseResult::Ok(state) => state,
            ReceiveResponseResult::Err(_) => panic!("unwrap called on Err"),
        }
    }
}

impl SnowInitiator {
    pub fn new(keypair: &SnowKeypair, remote_public_key: &SnowPublicKey) -> SnowInitiator {
        let mut state = keypair
            .builder()
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
    pub fn receive_response(mut self, response: SnowResponse) -> ReceiveResponseResult {
        let mut buf = [0u8; 65535];
        let result = self.0.read_message(&response.0, &mut buf);
        if result.is_err() {
            log::error!("Decryption error while trying to establish session: {}", result.err().unwrap().to_string()); 
            return ReceiveResponseResult::Err(self);
        }
        ReceiveResponseResult::Ok(LossyTransportState::from_stateless(self.0.into_stateless_transport_mode().unwrap()))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SnowResponse(Vec<u8>);

pub struct SnowResponder(HandshakeState);

impl SnowResponder {
    pub fn new(keypair: &SnowKeypair, remote_public_key: &SnowPublicKey) -> SnowResponder {
        let state = keypair
            .builder()
            .remote_public_key(&remote_public_key.0[..])
            .build_responder()
            .unwrap();
        SnowResponder(state)
    }
    pub fn response(
        mut self,
        initiation: SnowInitiation,
    ) -> Result<(LossyTransportState, SnowResponse), snow::Error> {
        let mut temp = [0u8; 65535];
        self.0.read_message(&initiation.0, &mut temp)?;
        let mut buf = [0u8; 65535];
        let len = self.0.write_message(&[], &mut buf)?;
        let _message = SnowResponse(buf[..len].to_vec());
        Ok((
            LossyTransportState::from_stateless(self.0.into_stateless_transport_mode()?),
            SnowResponse(buf[..len].to_vec()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{SnowInitiator, SnowKeypair, SnowResponder};

    #[tokio::test]
    async fn test_initiator_to_responder() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation).unwrap();
        let mut transport1 = initiator.receive_response(response).unwrap();

        let data = vec![1, 2, 3, 4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport2
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5, 6, 7, 8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport2
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[tokio::test]
    async fn test_responder_to_initiator() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation).unwrap();
        let mut transport1 = initiator.receive_response(response).unwrap();

        let data = vec![1, 2, 3, 4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport1
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5, 6, 7, 8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport1
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[tokio::test]
    async fn test_bidirectional() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation).unwrap();
        let mut transport1 = initiator.receive_response(response).unwrap();

        let data = vec![1, 2, 3, 4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport2
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5, 6, 7, 8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport1
            .read_message(&encrypted_buf[..len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[tokio::test]
    #[should_panic]
    async fn test_nonce_rejects() {
        let keypair1 = SnowKeypair::new();
        let keypair2 = SnowKeypair::new();

        let initiator = SnowInitiator::new(&keypair1, &keypair2.public());
        let responder = SnowResponder::new(&keypair2, &keypair1.public());

        let initiation = initiator.initiation();
        let (mut transport2, response) = responder.response(initiation).unwrap();
        let mut transport1 = initiator.receive_response(response).unwrap();

        let data = vec![1, 2, 3, 4];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];

        let encrypted_len = transport1
            .write_message(&data, &mut encrypted_buf)
            .await
            .unwrap();
        let len = transport2
            .read_message(&encrypted_buf[..encrypted_len], &mut result_buf)
            .await
            .unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        transport2
            .read_message(&encrypted_buf[..encrypted_len], &mut result_buf)
            .await
            .unwrap();
    }
}
