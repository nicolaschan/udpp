use std::{io::{BufRead, Read, Write}, net::SocketAddr};

use clap::Parser;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use veq::veq::{VeqSocket, ConnectionInfo};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, default_value_t = 0)]
    port: u64,
    #[clap(long)]
    public_ip: Option<SocketAddr>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionData {
    id: Uuid,
    conn_info: ConnectionInfo,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let mut socket = VeqSocket::bind(format!("0.0.0.0:{}", args.port)).await.unwrap();

    let mut conn_info = socket.connection_info();
    if let Some(ip) = args.public_ip {
        conn_info.addresses.push(ip);
    }
    let id = Uuid::from_u128(0);
    let connection_data = ConnectionData { id, conn_info };
    let conn_data_serialized = bincode::serialize(&connection_data).unwrap();
    let conn_data_b64 = base64::encode(&conn_data_serialized);

    println!("Your connection string: {}", conn_data_b64);
    print!("Enter remote connection string: ");
    std::io::stdout().flush().unwrap();

    let stdin = std::io::stdin();
    let mut peer_data_b64 = String::new();
    stdin.lock().read_line(&mut peer_data_b64).unwrap();
    let peer_data_b64 = peer_data_b64.strip_suffix("\n").unwrap();
    let peer_data_serialized = base64::decode(peer_data_b64).unwrap();
    let peer_data = bincode::deserialize::<ConnectionData>(&peer_data_serialized[..]).unwrap();

    let mut conn = socket.connect(peer_data.id, peer_data.conn_info).await;
    println!("connected");

    let mut conn_clone = conn.clone();
    tokio::spawn(async move {
        let mut stdout = std::io::stdout();
        loop {
            let received = conn_clone.recv().await.unwrap();
            stdout.write_all(&received[..]).unwrap();
            stdout.flush().unwrap();
        }
    });

    for line in stdin.lock().lines() {
        let mut data = Vec::new();
        line.unwrap().as_bytes().read_to_end(&mut data).unwrap();
        conn.send(data).await.unwrap();
    }
}