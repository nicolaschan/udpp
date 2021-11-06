use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = [u8; 32];

#[derive(Debug, Deserialize, Serialize)]
pub struct UdppPacket {
    pub session_id: Uuid,
    pub payload: UdppPayload,
}

impl UdppPacket {
    pub fn handshake(session_id: Uuid, data: Vec<u8>) -> UdppPacket {
        UdppPacket {
            session_id,
            payload: UdppPayload::Handshake(data),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UdppPayload {
    Handshake(Vec<u8>),
    Encrypted(Vec<u8>),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CleartextPayload {
    pub index: u64,
    pub content: UdppContent,
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UdppContent {
    Congestion(UdppCongestionMessage),
    DataFragment {
        group_index: u64,
        fragment_index: u64,
        number_of_fragments: u64,
        payload: Vec<u8>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UdppCongestionMessage {
    CongestionInformation(u128, CongestionReport),
    CongestionAcknowledgement(u128),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CongestionReport {
    start_index: u64,
    end_index: u64,
    number_accepted: u64,
}

impl CongestionReport {
    pub fn new(start_index: u64, end_index: u64, number_accepted: u64) -> CongestionReport {
        CongestionReport {
            start_index, end_index, number_accepted
        }
    }
    pub fn ratio(&self) -> (u64, u64) {
        (self.number_accepted, (self.end_index - self.start_index))
    }
}