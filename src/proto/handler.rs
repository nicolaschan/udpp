use std::{cmp::max, collections::{BTreeMap, HashMap, HashSet, VecDeque}, future::Future, net::SocketAddr, sync::{Arc, Mutex}, task::{Poll, Waker}, time::{SystemTime, UNIX_EPOCH}};

use async_std::{channel::{Receiver, Sender, unbounded}};
use snow::{HandshakeState, Keypair, TransportState};
use uuid::Uuid;


use crate::{proto::structs::UdppContent, transport::{AddressedSender}};

use super::structs::{CleartextPayload, CongestionReport, UdppCongestionMessage, UdppPacket, UdppPayload};

static NOISE_PARAMS: &str = "Noise_IX_25519_ChaChaPoly_BLAKE2s";

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
        let fragments = vec![None; length as usize];
        println!("fragment collection fragments init: {:?}", fragments);
        FragmentCollection {
            length,
            size: 0,
            fragments,
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
        println!("fragment recv fragments: {:?}", self.fragments);
        for (fragment, _index) in self.fragments.iter_mut().zip(1..self.length + 1) {
            match fragment {
                Some(chunk) => { data.append(chunk); },
                None => { return None; },
            }
        }
        println!("fragment recv data: {:?}", data);
        Some(data)
    }
}

#[derive(Debug)]
pub struct CongestionMonitor {
    pub start_index: u64, // index after which to report (inclusive)
    pub end_index: u64,  // latest index received (exclusive)
    pub number_accepted: u64,
}

impl Default for CongestionMonitor {
    fn default() -> Self {
        Self::new()
    }
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

#[derive(Debug)]
pub struct Session {
    pub session_id: Uuid,
    pub remote_address: SocketAddr,
    pub noise_state: NoiseState,
    pub payload_buffer: Arc<Mutex<VecDeque<Vec<u8>>>>,
    pub fragments: BTreeMap<u64, FragmentCollection>,
    pub incoming_congestion: CongestionMonitor,
    pub outgoing_congestion: CongestionReport,
    pub latency: Option<u128>,
    pub next_index: u64,
    pub next_group_index: u64,
    pub fragment_size: usize,
}

// impl std::fmt::Debug for Session {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         f.debug_struct("Session")
//             .field("remote_address", &self.remote_address)
//             .field("noise_state", &self.noise_state)
//             .field("payload_sender", &self.payload_queue)
//             .field("fragments", &self.fragments)
//             .field("incoming_congestion", &self.incoming_congestion)
//             .field("outgoing_congestion", &self.outgoing_congestion)
//             .field("latency", &self.latency)
//             .finish()
//     }
// }

fn unix_time() -> u128 {
    let start = SystemTime::now();
    let unix_time = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    unix_time.as_millis()
}

impl Session {
    pub fn new(session_id: Uuid, remote_address: SocketAddr, noise_state: NoiseState) -> Session {
        Session { 
            session_id,
            remote_address, 
            noise_state, 
            payload_buffer: Arc::new(Mutex::new(VecDeque::new())),
            fragments: BTreeMap::new(),
            incoming_congestion: CongestionMonitor::new(),
            outgoing_congestion: CongestionReport::new(0, 0, 0),
            latency: None,
            next_index: 0,
            next_group_index: 0,
            fragment_size: 1024,
        }
    }
    pub fn is_ready(&self) -> bool {
        match self.noise_state {
            NoiseState::Handshake(_) => false,
            NoiseState::Transport(_) => true,
        }
    }
    pub fn send_content(&mut self, content: UdppContent) -> Option<(SocketAddr, UdppPacket)> {
        let payload = CleartextPayload {
            index: self.next_index,
            content,
        };
        self.next_index += 1;
        let serialized_payload = bincode::serialize(&payload).unwrap();
        let mut encrypted_payload = [0u8; 65535];
        if let NoiseState::Transport(transport) = &mut self.noise_state {
            let len = transport.write_message(&serialized_payload, &mut encrypted_payload).unwrap();
            let encrypted_payload_vec = encrypted_payload[..len].to_vec();

            let packet = UdppPacket {
                session_id: self.session_id,
                payload: UdppPayload::Encrypted(encrypted_payload_vec),
            };

            return Some((self.remote_address, packet));
        }
        None
    }
    pub fn send_data(&mut self, data: Vec<u8>) -> Vec<(SocketAddr, UdppPacket)> {
        let chunks = data.chunks(self.fragment_size);
        let number_of_fragments = chunks.len() as u64;
        let group_index = self.next_group_index;
        self.next_group_index += 1;
        let fragments = chunks.enumerate().map(|(fragment_index, chunk)| {
            UdppContent::DataFragment {
                group_index,
                fragment_index: fragment_index as u64,
                number_of_fragments,
                payload: chunk.to_vec(),
            }
        });
        fragments.filter_map(|fragment| self.send_content(fragment)).collect()
    }
    pub fn handle_payload(&mut self, payload: CleartextPayload) {
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
                println!("recv datafragment");
                self.fragments.entry(group_index).or_insert_with(|| FragmentCollection::new(number_of_fragments));
                if let Some(collection) = self.fragments.get_mut(&group_index) {
                    collection.insert_fragment(fragment_index, payload);
                    println!("recv datafragment - insert collection");
                    if collection.is_ready() {
                        println!("recv datafragment - collection ready");
                        if let Some(collection) = self.fragments.remove(&group_index) {
                            println!("recv datafragment - get collection {:?}", collection);
                            if let Some(data) = collection.recv() {
                                println!("recv datafragment - recv collection");
                                let mut payload_buffer_guard= self.payload_buffer.lock().unwrap();
                                payload_buffer_guard.push_back(data);
                                println!("recv datafragment - inserted payload buffer");
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
    pub fn try_recv(&mut self) -> Option<Vec<u8>> {
        let mut payload_buffer_guard = self.payload_buffer.lock().unwrap();
        payload_buffer_guard.pop_front()
    }
}

pub struct UdppHandler {
    pub sessions: HashMap<Uuid, Session>,
    pub ready_sessions: Arc<Mutex<HashSet<Uuid>>>,
    pub wakers: Arc<Mutex<HashMap<Uuid, Waker>>>,
    pub new_sessions_sender: Sender<Uuid>,
    pub new_sessions_receiver: Receiver<Uuid>,
    pub keypair: Keypair,
    pub sender: Box<dyn AddressedSender + Send>,
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
    pub fn new(sender: Box<dyn AddressedSender + Send>) -> UdppHandler {
        let builder = snow::Builder::new(NOISE_PARAMS.parse().unwrap());
        let keypair = builder.generate_keypair().unwrap();
        let (new_sessions_sender, new_sessions_receiver) = unbounded();
        UdppHandler {
            sessions: HashMap::new(),
            wakers: Arc::new(Mutex::new(HashMap::new())),
            ready_sessions: Arc::new(Mutex::new(HashSet::new())),
            new_sessions_sender,
            new_sessions_receiver,
            keypair,
            sender, 
        }
    }

    pub fn new_sessions_stream(&mut self) -> Receiver<Uuid> {
        self.new_sessions_receiver.clone()
    }

    async fn new_session_started(&mut self, session_id: Uuid) {
        self.new_sessions_sender.send(session_id).await.unwrap();
        {
            let mut ready_sessions_guard = self.ready_sessions.lock().unwrap();
            ready_sessions_guard.insert(session_id);
        }
        {
            let wakers_guard = self.wakers.lock().unwrap();
            if let Some(waker) = wakers_guard.get(&session_id) {
                waker.wake_by_ref();
            }
        }
    }

    async fn handshake(&mut self, remote_address: SocketAddr, session_id: Uuid, incoming_message: Vec<u8>) -> Result<Option<UdppPacket>, snow::Error> {
        let responder = match self.sessions.remove(&session_id) {
            Some(session) => session.noise_state,
            None => NoiseState::Handshake(snow::Builder::new(NOISE_PARAMS.parse()?)
                .local_private_key(&self.keypair.private)
                .build_responder()?),
        };
        match responder {
            NoiseState::Handshake(mut handshake_state) => {
                let mut read_buf = [0u8; 1024];
                handshake_state.read_message(&incoming_message[..], &mut read_buf).unwrap();

                let mut return_value = None;
                if handshake_state.is_my_turn() {
                    let mut response_buf = [0u8; 1024];
                    if let Ok(len) = handshake_state.write_message(&[], &mut response_buf) {
                        return_value = Some(UdppPacket::handshake(session_id, response_buf[..len].to_vec()));
                    };
                }
                if handshake_state.is_handshake_finished() {
                    let transport_state = handshake_state.into_transport_mode()?;
                    let session = Session::new(session_id, remote_address, NoiseState::Transport(transport_state));
                    self.sessions.insert(session_id, session);
                    self.new_session_started(session_id).await;
                } else {
                    let session = Session::new(session_id, remote_address, NoiseState::Handshake(handshake_state));
                    self.sessions.insert(session_id, session);
                }
                Ok(return_value)
            },
            NoiseState::Transport(transport) => {
                let session = Session::new(session_id, remote_address, NoiseState::Transport(transport));
                self.sessions.insert(session_id, session);
                Ok(None)
            },
        }
    }

    pub async fn establish_session(&mut self, remote_address: SocketAddr) -> SessionFuture {
        let session_id = Uuid::new_v4();
        let mut initiator = snow::Builder::new(NOISE_PARAMS.parse().unwrap())
            .local_private_key(&self.keypair.private)
            .build_initiator()
            .unwrap();
        let mut response_buf = [0u8; 1024];
        let len = initiator.write_message(&[], &mut response_buf).unwrap();
        let session = Session::new(session_id, remote_address, NoiseState::Handshake(initiator));
        self.sessions.insert(session_id, session);
        let handshake_packet = UdppPacket::handshake(session_id, response_buf[..len].to_vec());
        self.send_packet(remote_address, handshake_packet).await;
        let wakers = self.wakers.clone();
        SessionFuture {
            session_id,
            ready_sessions: self.ready_sessions.clone(),
            wakers,
        }
    }

    pub async fn send_packet(&mut self, addr: SocketAddr, packet: UdppPacket) {
        let serialized = bincode::serialize(&packet).unwrap();
        self.sender.addressed_send(addr, serialized).await.unwrap();
    }

    pub async fn send_data(&mut self, session_id: Uuid, data: Vec<u8>) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            for (addr, packet) in session.send_data(data) {
                self.send_packet(addr, packet).await;
            }
        }
    }
    pub fn try_recv_session(&mut self, session_id: Uuid) -> Option<Vec<u8>> {
        self.sessions.get_mut(&session_id)
            .and_then(|session| session.try_recv())
    }

    pub fn read_packet(data: Vec<u8>) -> Result<UdppPacket, Box<bincode::ErrorKind>> {
        bincode::deserialize(&data[..])
    }

    pub async fn handle_incoming(&mut self, src: SocketAddr, data: Vec<u8>) -> Option<Uuid> {
        if let Ok(UdppPacket { session_id, payload }) = UdppHandler::read_packet(data) {
            match payload {
                UdppPayload::Handshake(payload) => {
                    match self.handshake(src, session_id, payload).await {
                        Ok(None) => { /* nothing */}
                        Ok(Some(packet)) => {
                            self.send_packet(src, packet).await;
                        },
                        Err(e) => { eprintln!("{:?}", e); },
                    }
                    if let Some(waker) = self.wakers.lock().unwrap().get(&session_id) {
                        waker.wake_by_ref();
                    }
                },
                UdppPayload::Encrypted(encrypted_payload) => {
                    if let Some(session) = self.sessions.get_mut(&session_id) {
                        match &mut session.noise_state {
                            NoiseState::Handshake(_) => { /* handshake not complete */},
                            NoiseState::Transport(transport_state) => {
                                let mut read_buf = [0u8; 65535];
                                if let Ok(len) = transport_state.read_message(&encrypted_payload[..], &mut read_buf) {
                                    if let Ok(payload) = bincode::deserialize::<CleartextPayload>(&read_buf[..len]) {
                                        session.handle_payload(payload);
                                        let wakers_guard = self.wakers.lock().unwrap();
                                        if let Some(waker) = wakers_guard.get(&session_id) {
                                            waker.wake_by_ref();
                                        }
                                    }
                                }
                            },
                        }
                    }
                },
            }
        };
        None
    }
}

pub struct SessionFuture {
    pub session_id: Uuid,
    pub ready_sessions: Arc<Mutex<HashSet<Uuid>>>,
    pub wakers: Arc<Mutex<HashMap<Uuid, Waker>>>,
}

impl Future for SessionFuture {
    type Output = Result<Uuid, std::io::Error>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let mut waker_guard = self.wakers.lock().unwrap();
        let sessions_guard = self.ready_sessions.lock().unwrap();
        if sessions_guard.contains(&self.session_id) {
            waker_guard.remove(&self.session_id);
            return Poll::Ready(Ok(self.session_id));
        }
        waker_guard.insert(self.session_id, cx.waker().clone());
        Poll::Pending
    }
}