use crossbeam::channel::unbounded;

use rand::RngCore;
use crossbeam::channel::{Sender, Receiver};
use std::net::SocketAddr;

use std::sync::Arc;
use std::sync::Mutex;


use serde::{Deserialize, Serialize};
use std::net::UdpSocket;


use std::result::Result;
use std::io::Error;
use std::collections::HashMap;


#[cfg(test)]
mod tests {
    use crossbeam::channel::unbounded;

    use crate::{UdppListener};

    #[test]
    fn test_listen_and_connect() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();

        let mut stream_2_1 = listener2.connect(listener1.local_addr().unwrap()).unwrap();
        let data: Vec<u8> = vec![42, 43, 44, 45];
        stream_2_1.send(&data).unwrap();

        let mut stream_1_2 = listener1.incoming().next().unwrap();
        let packet = stream_1_2.recv().unwrap();
        assert_eq!(packet, data);
    }

    #[test]
    fn test_congestion() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();

        let mut stream_2_1 = listener2.connect(listener1.local_addr().unwrap()).unwrap();
        let data: Vec<u8> = vec![42, 43, 44, 45];
        stream_2_1.send(&data).unwrap();

        let mut stream_1_2 = listener1.incoming().next().unwrap();
        let packet = stream_1_2.recv().unwrap();
        assert_eq!(packet, data);
        assert_eq!(stream_1_2.congestion(), 0.0);
    }

    #[test]
    fn test_bidirectional_communication() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();

        let mut stream_2_1 = listener2.connect(listener1.local_addr().unwrap()).unwrap();
        let data: Vec<u8> = vec![42, 43, 44, 45];
        stream_2_1.send(&data).unwrap();

        let mut stream_1_2 = listener1.incoming().next().unwrap();
        let packet = stream_1_2.recv().unwrap();
        assert_eq!(packet, data);

        let data2: Vec<u8> = vec![10, 9, 8, 7];
        stream_1_2.send(&data2).unwrap();
        let packet2 = stream_2_1.recv().unwrap();
        assert_eq!(packet2, data2);
    }

    #[test]
    fn test_multichannel() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();

        let mut channel1= listener2.connect(listener1.local_addr().unwrap()).unwrap();
        let mut channel2= listener2.connect(listener1.local_addr().unwrap()).unwrap();

        let data1: Vec<u8> = vec![10, 20, 30, 40];
        let data2: Vec<u8> = vec![11, 12, 13, 14];
        channel1.send(&data1).unwrap();
        channel2.send(&data2).unwrap();

        let mut incoming = listener1.incoming();
        let mut channel1_recv = incoming.next().unwrap();
        let mut channel2_recv = incoming.next().unwrap();

        assert_eq!(channel1_recv.recv().unwrap(), data1);
        assert_eq!(channel2_recv.recv().unwrap(), data2);
    }

    #[test]
    fn test_multiple_messages() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let mut channel1= listener2.connect(listener1.local_addr().unwrap()).unwrap();

        let data1: Vec<u8> = vec![1, 2, 3, 4];
        let data2: Vec<u8> = vec![5, 6, 7, 8];
        channel1.send(&data1).unwrap();
        channel1.send(&data2).unwrap();
        let mut channel1_recv = listener1.incoming().next().unwrap();

        assert_eq!(channel1_recv.recv().unwrap(), data1);
        assert_eq!(channel1_recv.recv().unwrap(), data2);
    }

    #[test]
    fn test_multithread_send_receive() {
        let (address_sender, address_receiver) = unbounded();
        let thread = std::thread::spawn(move || {
            let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
            let local_addr = listener1.local_addr().unwrap().clone();
            address_sender.send(local_addr).unwrap();
            let mut channel1_recv = listener1.incoming().next().unwrap();
            let packet = channel1_recv.recv().unwrap();
            assert_eq!(packet, vec![1, 2, 3, 4]);
        });
        let addr = address_receiver.recv().unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let mut channel1 = listener2.connect(addr).unwrap();
        channel1.send(&vec![1, 2, 3, 4]).unwrap();
        thread.join().unwrap();
    }

    #[test]
    fn test_big_messages() {
        let listener1 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let listener2 = UdppListener::bind("127.0.0.1:0".parse().unwrap()).unwrap();

        let mut stream_2_1 = listener2.connect(listener1.local_addr().unwrap()).unwrap();
        let data  = [42; 50000];
        stream_2_1.send(&data.to_vec()).unwrap();

        let mut stream_1_2 = listener1.incoming().next().unwrap();
        let packet = stream_1_2.recv().unwrap();
        assert_eq!(packet, data);
    }
}

type SessionId = [u8; 32];

#[derive(Debug, Deserialize, Serialize)]
pub struct UdppPacket {
    pub session_id: SessionId,
    pub index: u64,
    pub data: Vec<u8>,
}

pub struct UdppListener {
    pub stream_receiver: Receiver<UdppStream>,
    pub udp_socket: UdpSocket,
    pub streams: Arc<Mutex<HashMap<SessionId, Sender<UdppPacket>>>>,
}

impl UdppListener {
    pub fn local_addr(&self) -> Result<SocketAddr, Error> {
        self.udp_socket.local_addr()
    }
    pub fn bind(addr: SocketAddr) -> Result<UdppListener, Error> {
        let (stream_sender, stream_receiver) = unbounded();
        let udp_socket = UdpSocket::bind(addr).unwrap();
        let udp_socket_clone = udp_socket.try_clone().unwrap();
        let streams: Arc<Mutex<HashMap<SessionId, Sender<UdppPacket>>>> = Arc::new(Mutex::new(HashMap::new()));
        let streams_clone = streams.clone();

        std::thread::spawn(move || {
            let mut buf = [0u8; 65536];
            while let Ok((number_of_bytes, src_addr)) = udp_socket.recv_from(&mut buf) {
                if let Ok(packet) = bincode::deserialize::<UdppPacket>(&buf[..number_of_bytes]) {
                    let mut streams_guard = streams.lock().unwrap();
                    let stream = streams_guard.get_mut(&packet.session_id);
                    match stream {
                        Some(sender) => { sender.send(packet).unwrap(); },
                        None => {
                            let (packet_sender, packet_receiver) = unbounded();
                            let (payload_sender, payload_receiver) = unbounded();
                            let stream = UdppStream::new(packet.session_id, payload_receiver, packet_sender.clone());
                            streams_guard.insert(packet.session_id.clone(), payload_sender.clone());
                            payload_sender.send(packet).unwrap();
                            stream_sender.send(stream).unwrap();
                            UdppListener::register_packet_receiver(udp_socket.try_clone().unwrap(), src_addr, packet_receiver.clone());
                        },
                    };
                }
            }
        });

        let listener = UdppListener {
            stream_receiver,
            udp_socket: udp_socket_clone,
            streams: streams_clone,
        };
        Ok(listener)
    }
    pub fn incoming(self) -> IncomingUdppStreams {
        IncomingUdppStreams {
            udpp_listener: self,
        }
    }
    pub fn register_packet_receiver(udp_socket: UdpSocket, addr: SocketAddr, packet_receiver: Receiver<UdppPacket>) {
        std::thread::spawn(move || {
            while let Ok(packet) = packet_receiver.recv() {
                if let Ok(serialized) = bincode::serialize(&packet) {
                    if let Ok(_) = udp_socket.send_to(&serialized, addr) {}
                }
            }
        });
    }
    pub fn connect(&self, addr: SocketAddr) -> Result<UdppStream, Error> {
        let mut session_id= [0u8; 32];
        let mut rng = rand::thread_rng();
        rng.fill_bytes(&mut session_id);
        let (packet_sender, packet_receiver) = unbounded();
        let (payload_sender, payload_receiver) = unbounded();

        let mut streams_guard= self.streams.lock().unwrap();
        streams_guard.insert(session_id, payload_sender);

        let udp_socket = self.udp_socket.try_clone().unwrap();
        UdppListener::register_packet_receiver(udp_socket, addr, packet_receiver);
        Ok(UdppStream::new(session_id, payload_receiver, packet_sender))
    }
}

pub struct IncomingUdppStreams {
    udpp_listener: UdppListener,
}

impl Iterator for IncomingUdppStreams {
    type Item = UdppStream;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        self.udpp_listener.stream_receiver.recv().ok()
    }
}

pub struct UdppStream {
    session_id: SessionId,
    last_received_sequence_number: u64,
    recevied_packet_count: u64,
    sent_sequence_number: u64,
    receiver: Receiver<UdppPacket>,
    sender: Sender<UdppPacket>,
}

impl UdppStream {
    pub fn new(session_id: SessionId, receiver: Receiver<UdppPacket>, sender: Sender<UdppPacket>) -> UdppStream {
        UdppStream {
            session_id,
            last_received_sequence_number: 0,
            recevied_packet_count: 0,
            sent_sequence_number: 0,
            receiver,
            sender,
        }
    }
    pub fn send(&mut self, data: &Vec<u8>) -> Result<(), Error> {
        self.sent_sequence_number += 1;
        let packet = UdppPacket {
            session_id: self.session_id,
            index: self.sent_sequence_number,
            data: data.to_vec(),
        };
        self.sender.send(packet).unwrap();
        Ok(())
    }
    pub fn recv(&mut self) -> Result<Vec<u8>, crossbeam::channel::RecvError> {
        let packet = self.receiver.recv()?;
        self.last_received_sequence_number =  packet.index;
        self.recevied_packet_count += 1;
        Ok(packet.data)
    }
    pub fn congestion(&self) -> f64 {
        ((self.last_received_sequence_number - self.recevied_packet_count) as f64) / (self.last_received_sequence_number as f64)
    }
}
