use std::io::{BufRead, Read, Write};

use clap::Parser;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use veq::veq::{ConnectionInfo, VeqSocket};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(long, default_value_t = 0)]
    port: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConnectionData {
    id: Uuid,
    conn_info: ConnectionInfo,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let mut socket = VeqSocket::bind(format!("0.0.0.0:{}", args.port))
        .await
        .unwrap();

    let conn_info = socket.connection_info();
    let id = Uuid::from_u128(0);
    let connection_data = ConnectionData { id, conn_info };
    let conn_data_serialized = bincode::serialize(&connection_data).unwrap();
    let mut conn_data_compressed = Vec::new();
    zstd::stream::copy_encode(&conn_data_serialized[..], &mut conn_data_compressed, 10).unwrap();
    let conn_data_emoji = base64::encode(&conn_data_compressed);

    eprintln!("Your data: {:?}", connection_data);
    eprintln!("Your connection string:\n{}", conn_data_emoji);
    eprint!("Enter remote connection string: ");
    std::io::stdout().flush().unwrap();

    let stdin = std::io::stdin();
    let mut peer_data_emoji = String::new();
    stdin.lock().read_line(&mut peer_data_emoji).unwrap();
    let peer_data_emoji = peer_data_emoji.replace('\n', "").replace(' ', "");
    let peer_data_compressed = base64::decode(peer_data_emoji).unwrap();
    let mut peer_data_serialized = Vec::new();
    zstd::stream::copy_decode(&peer_data_compressed[..], &mut peer_data_serialized).unwrap();
    let peer_data = bincode::deserialize::<ConnectionData>(&peer_data_serialized[..]).unwrap();
    eprintln!("Remote connection string accepted");

    let mut conn = socket.connect(peer_data.id, peer_data.conn_info).await;
    eprintln!("Connected to {}", conn.remote_addr().await);

    let mut conn_clone = conn.clone();
    tokio::spawn(async move {
        let mut stdout = std::io::stdout();
        loop {
            let received = conn_clone.recv().await.unwrap();
            stdout.write_all(&received[..]).unwrap();
            stdout.flush().unwrap();
            println!();
        }
    });

    for line in stdin.lock().lines() {
        let mut data = Vec::new();
        line.unwrap().as_bytes().read_to_end(&mut data).unwrap();
        conn.send(data).await.unwrap();
    }
}
