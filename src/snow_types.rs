use std::hash::Hash;

use serde::{Serialize, Deserialize};
use snow::{Builder, Keypair, HandshakeState, TransportState};

static NOISE_PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";
pub fn builder<'a>() -> Builder<'a> {
    snow::Builder::new(NOISE_PARAMS.parse().unwrap())
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
    pub fn receive_response(mut self, response: SnowResponse) -> TransportState {
        let mut buf = [0u8; 65535];
        self.0.read_message(&response.0, &mut buf).unwrap();
        self.0.into_transport_mode().unwrap()
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
    pub fn response(mut self, initiation: SnowInitiation) -> (TransportState, SnowResponse) {
        let mut temp = [0u8; 65535];
        self.0.read_message(&initiation.0, &mut temp).unwrap();
        let mut buf = [0u8; 65535];
        let len = self.0.write_message(&[], &mut buf).unwrap();
        let message = SnowResponse(buf[..len].to_vec());
        (self.0.into_transport_mode().unwrap(), SnowResponse(buf[..len].to_vec()))
    }
}


#[cfg(test)]
mod tests {
    use super::{SnowKeypair, SnowInitiator, SnowResponder};

    #[test]
    fn test_initiator_to_responder() {
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
        let len = transport1.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport2.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport1.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport2.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[test]
    fn test_responder_to_initiator() {
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
        let len = transport2.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport1.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport1.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }

    #[test]
    fn test_bidirectional() {
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
        let len = transport1.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport2.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);

        let data = vec![5,6,7,8];
        let mut encrypted_buf = [0u8; 65535];
        let mut result_buf = [0u8; 65535];
        let len = transport2.write_message(&data, &mut encrypted_buf).unwrap();
        let len = transport1.read_message(&encrypted_buf[..len], &mut result_buf).unwrap();
        assert_eq!(&data[..], &result_buf[..len]);
    }
}