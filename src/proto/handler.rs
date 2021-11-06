use std::{cmp::max, collections::{BTreeMap, HashMap, VecDeque}, net::SocketAddr, slice::SliceIndex, sync::{Arc, Mutex}, time::{Instant, SystemTime, UNIX_EPOCH}};

use bincode::Options;
use chacha20poly1305::{Key, aead::generic_array::typenum::private::IsEqualPrivate};
use ed25519::signature::{Signature, Signer};
use rand::{CryptoRng, Rng, rngs::OsRng};
use snow::{HandshakeState, Keypair, TransportState, params::NoiseParams};
use uuid::Uuid;
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::{proto::structs::UdppContent, transport::{Sink, Transport}};

use super::structs::{CleartextPayload, CongestionReport, SessionId, UdppCongestionMessage, UdppPacket, UdppPayload};

static NOISE_PARAMS: &'static str = "Noise_IX_25519_ChaChaPoly_BLAKE2s";

#[derive(Debug)]
pub enum NoiseState {
    Handshake(HandshakeState),
    Transport(TransportState),
}

#[derive(Debug)]
pub struct FragmentCollection {
    length: u64,
    size: u64,
    fragments: Vec<Option<Vec<u8>>>,
}

impl FragmentCollection {
    pub fn new(length: u64) -> FragmentCollection {
        FragmentCollection {
            length,
            size: 0,
            fragments: vec![None; length as usize],
        }
    }

    pub fn insert_fragment(&mut self, fragment_index: u64, data: Vec<u8>) {
        self.fragments.insert(fragment_index as usize, Some(data));
        self.size += 1;
    }

    pub fn is_ready(&self) -> bool {
        self.length == self.size
    }

    pub fn recv(mut self) -> Option<Vec<u8>> {
        let mut data: Vec<u8> = Vec::new();
        for fragment in self.fragments.iter_mut() {
            match fragment {
                Some(chunk) => { data.append(chunk); },
                None => { return None; },
            }
        }
        Some(data)
    }
}

#[derive(Debug)]
pub struct CongestionMonitor {
    pub start_index: u64, // index after which to report (inclusive)
    pub end_index: u64,  // latest index received (exclusive)
    pub number_accepted: u64,
}

impl CongestionMonitor {
    pub fn new() -> CongestionMonitor {
        CongestionMonitor {
            start_index: 0,
            end_index: 0,
            number_accepted: 0,
        }
    }
    pub fn accept_payload(&mut self, index: u64) {
        self.end_index = max(self.end_index, index + 1);
        self.number_accepted += 1;
    }
    pub fn report(&mut self) -> CongestionReport {
        let report = CongestionReport::new(self.start_index, self.end_index, self.number_accepted);
        self.start_index = self.end_index;
        self.number_accepted = 0;
        report
    }
}

pub struct Session {
    pub session_id: Uuid,
    pub remote_address: SocketAddr,
    pub noise_state: NoiseState,
    pub payload_queue: VecDeque<Vec<u8>>,
    pub fragments: BTreeMap<u64, FragmentCollection>,
    pub incoming_congestion: CongestionMonitor,
    pub outgoing_congestion: CongestionReport,
    pub latency: Option<u128>,
    pub sink: Arc<Mutex<Box<dyn Sink>>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("remote_address", &self.remote_address)
            .field("noise_state", &self.noise_state)
            .field("payload_queue", &self.payload_queue)
            .field("fragments", &self.fragments)
            .field("incoming_congestion", &self.incoming_congestion)
            .field("outgoing_congestion", &self.outgoing_congestion)
            .field("latency", &self.latency)
            .finish()
    }
}

fn unix_time() -> u128 {
    let start = SystemTime::now();
    let unix_time = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    unix_time.as_millis()
}

impl Session {
    pub fn new(session_id: Uuid, remote_address: SocketAddr, noise_state: NoiseState, sink: Arc<Mutex<Box<dyn Sink>>>) -> Session {
        Session { 
            session_id,
            remote_address, 
            noise_state, 
            payload_queue: VecDeque::new(), 
            fragments: BTreeMap::new(),
            incoming_congestion: CongestionMonitor::new(),
            outgoing_congestion: CongestionReport::new(0, 0, 0),
            latency: None,
            sink,
        }
    }
    pub fn send_content(&mut self, content: UdppContent) {
        let content_payload = bincode::serialize(&content).unwrap();
        let mut encrypted_payload = [0u8; 65535];
        if let NoiseState::Transport(ref mut transport) = (*self).noise_state {
            let len = transport.write_message(&content_payload, &mut encrypted_payload).unwrap();
            let encrypted_payload_vec = encrypted_payload[..len].to_vec();

            let packet = UdppPacket {
                session_id: self.session_id,
                payload: UdppPayload::Encrypted(encrypted_payload_vec),
            };

            let serialized = bincode::serialize(&packet).unwrap();
            if let Ok(sink_guard) = self.sink.lock() {
                sink_guard.send(self.remote_address, serialized);
            }
        }
    }
    pub fn add_payload(&mut self, payload: CleartextPayload) {
        self.incoming_congestion.accept_payload(payload.index);
        match payload.content {
            UdppContent::Congestion(congestion) => {
                match congestion {
                    UdppCongestionMessage::CongestionInformation(sent_timestamp, report) => {
                        self.outgoing_congestion = report;
                        self.send_content(UdppContent::Congestion(UdppCongestionMessage::CongestionAcknowledgement(sent_timestamp)));
                    },
                    UdppCongestionMessage::CongestionAcknowledgement(timestamp) => {
                        self.latency = Some(unix_time() - timestamp);
                    },
                }
            },
            UdppContent::DataFragment { group_index, fragment_index, number_of_fragments, payload } => {
                if !self.fragments.contains_key(&group_index) {
                    let fragment_collection = FragmentCollection::new(number_of_fragments);
                    self.fragments.insert(group_index, fragment_collection);
                }
                if let Some(collection) = self.fragments.get_mut(&group_index) {
                    collection.insert_fragment(fragment_index, payload);
                    if collection.is_ready() {
                        if let Some(collection) = self.fragments.remove(&group_index) {
                            if let Some(data) = collection.recv() {
                                self.payload_queue.push_back(data);
                            }
                        }
                    }
                }
            },
        }
    }
    pub fn incoming_congestion(&mut self) -> CongestionReport {
        self.incoming_congestion.report()
    }
    pub fn outgoing_congestion(&mut self) -> CongestionReport {
        todo!()
    }
    pub fn recv(&mut self) -> Option<Vec<u8>> {
        self.payload_queue.pop_front()
    }
}

pub struct UdppHandler {
    pub sessions: HashMap<Uuid, Session>,
    pub keypair: Keypair,
    pub sink: Arc<Mutex<Box<dyn Sink>>>,
}

impl std::fmt::Debug for UdppHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdppHandler")
            .field("sessions", &self.sessions)
            // .field("keypair", &self.keypair)
            // .field("sink", &self.sink)
            .finish()
    }
}

impl UdppHandler {
    pub fn new(sink: Box<dyn Sink>) -> UdppHandler {
        let builder = snow::Builder::new(NOISE_PARAMS.parse().unwrap());
        let keypair = builder.generate_keypair().unwrap();

        UdppHandler {
            sessions: HashMap::new(),
            keypair,
            sink: Arc::new(Mutex::new(sink)),
        }
    }

    fn handshake(&mut self, remote_address: SocketAddr, session_id: Uuid, incoming_message: Vec<u8>) -> Result<Option<UdppPacket>, snow::Error> {
        let responder = match self.sessions.remove(&session_id) {
            Some(session) => session.noise_state,
            None => NoiseState::Handshake(snow::Builder::new(NOISE_PARAMS.parse()?)
                .local_private_key(&self.keypair.private)
                .build_responder()?),
        };
        match responder {
            NoiseState::Handshake(mut handshake_state) => {
                let mut read_buf = [0u8; 1024];
                handshake_state.read_message(&incoming_message[..], &mut read_buf);

                let mut return_value = None;
                if handshake_state.is_my_turn() {
                    let mut response_buf = [0u8; 1024];
                    if let Ok(len) = handshake_state.write_message(&[], &mut response_buf) {
                        return_value = Some(UdppPacket::handshake(session_id, response_buf[..len].to_vec()));
                    };
                }
                if handshake_state.is_handshake_finished() {
                    let transport_state = handshake_state.into_transport_mode()?;
                    let session = Session::new(session_id, remote_address, NoiseState::Transport(transport_state), self.sink.clone());
                    self.sessions.insert(session_id, session);
                } else {
                    let session = Session::new(session_id, remote_address, NoiseState::Handshake(handshake_state), self.sink.clone());
                    self.sessions.insert(session_id, session);
                }
                Ok(return_value)
            },
            NoiseState::Transport(transport) => {
                let session = Session::new(session_id, remote_address, NoiseState::Transport(transport), self.sink.clone());
                self.sessions.insert(session_id, session);
                Ok(None)
            },
        }
    }

    pub fn establish_session(&mut self, remote_address: SocketAddr) -> Result<usize, std::io::Error> {
        let session_id = Uuid::new_v4();
        let mut initiator = snow::Builder::new(NOISE_PARAMS.parse().unwrap())
            .local_private_key(&self.keypair.private)
            .build_initiator()
            .unwrap();
        let mut response_buf = [0u8; 1024];
        let len = initiator.write_message(&[], &mut response_buf).unwrap();
        let session = Session::new(session_id, remote_address, NoiseState::Handshake(initiator), self.sink.clone());
        self.sessions.insert(session_id, session);
        let handshake_packet = UdppPacket::handshake(session_id, response_buf[..len].to_vec());
        self.send_packet(remote_address, handshake_packet)
    }

    pub fn send_packet(&mut self, addr: SocketAddr, packet: UdppPacket) -> Result<usize, std::io::Error> {
        let serialized = bincode::serialize(&packet).unwrap();
        let sink_guard = self.sink.lock().unwrap();
        sink_guard.send(addr, serialized)
    }

    pub fn send_content(&mut self, session_id: Uuid, content: UdppContent) -> Result<(), snow::Error> {
        if let Some(session) = self.sessions.remove(&session_id) {
            if let NoiseState::Transport(mut transport) = session.noise_state {
                let serialized = bincode::serialize(&content).unwrap();
                let mut data = [0u8; 65535];
                let len = transport.write_message(&serialized[..], &mut data)?;
                let packet = UdppPacket {
                    session_id,
                    payload: UdppPayload::Encrypted(data[..len].to_vec()),
                };
                self.send_packet(session.remote_address, packet);
                let session = Session::new(session_id, session.remote_address, NoiseState::Transport(transport), self.sink.clone());
                self.sessions.insert(session_id, session);
            }
        }
        Ok(())
    }

    pub fn read_packet(data: Vec<u8>) -> Result<UdppPacket, Box<bincode::ErrorKind>> {
        bincode::deserialize(&data[..])
    }

    pub fn handle_incoming(&mut self, src: SocketAddr, data: Vec<u8>) {
        match UdppHandler::read_packet(data) {
            Ok(UdppPacket { session_id, payload }) => {
                match payload {
                    UdppPayload::Handshake(payload) => {
                        match self.handshake(src, session_id, payload) {
                            Ok(None) => { /* nothing */}
                            Ok(Some(packet)) => { self.send_packet(src, packet); },
                            Err(e) => { eprintln!("{:?}", e); },
                        }
                    },
                    UdppPayload::Encrypted(encrypted_payload) => {
                        match self.sessions.get_mut(&session_id) {
                            Some(session) => {
                                match &mut session.noise_state {
                                    NoiseState::Handshake(_) => { /* handshake not complete */},
                                    NoiseState::Transport(transport_state) => {
                                        let mut read_buf = [0u8; 65535];
                                        match transport_state.read_message(&encrypted_payload[..], &mut read_buf) {
                                            Ok(len) => {
                                                if let Ok(payload) = bincode::deserialize::<CleartextPayload>(&read_buf[..len]) {
                                                    session.add_payload(payload);
                                                }
                                            },
                                            Err(_) => { /* failed to read */},
                                        }
                                    },
                                }
                            },
                            None => { /* can't decrypt! */}
                        }
                    },
                }
            },
            Err(_) => { /* invalid packet format */ }
        }
    }
}