#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![feature(map_first_last)]
#![feature(async_stream)]
#![feature(untagged_unions)]
#![feature(future_join)]
#![feature(future_poll_fn)]

use tokio::join;
use uuid::Uuid;

pub mod veq;
mod transport;
mod handler;
pub mod snow_types;
mod session;
mod ip_discovery;
mod transform;

#[cfg(test)]
mod tests {
    use tokio::join;
    use uuid::Uuid;

    use crate::{veq::{VeqSocket, VeqSession, BidirectionalSession, VeqSessionAlias}, transform::Chunker};

    async fn get_conns() -> (VeqSessionAlias, VeqSessionAlias) {
        let mut socket1 = VeqSocket::bind("0.0.0.0:0").await.unwrap();
        let mut socket2 = VeqSocket::bind("0.0.0.0:0").await.unwrap();

        let id = Uuid::new_v4();
        let info1 = socket1.connection_info();
        let info2 = socket2.connection_info();
        return join!(
            socket1.connect(id, info2),
            socket2.connect(id, info1),
        );
    }

    #[tokio::test]
    async fn test_bind_port() {
        let port = 54783;
        let socket = VeqSocket::bind(format!("0.0.0.0:{}", port)).await.unwrap();
        let connection_info = socket.connection_info();
        let bound_port = connection_info.addresses.into_iter().next().unwrap().port();
        assert_eq!(port, bound_port);
    }

    #[tokio::test]
    async fn test_bidirectional_small() {
        let (mut conn1, mut conn2) = get_conns().await;

        let data = vec![0,1,2,3];
        conn1.send(data.clone()).await.unwrap();
        let received = conn2.recv().await;
        assert_eq!(data, received.unwrap());

        let data2 = vec![5,6,7,8];
        conn2.send(data2.clone()).await.unwrap();
        let received2 = conn1.recv().await;
        assert_eq!(data2, received2.unwrap());
    }

    #[tokio::test]
    async fn test_queued_small() {
        let (mut conn1, mut conn2) = get_conns().await;
        let data1 = vec![0,1,2,3];
        let data2 = vec![5,4,3,2];
        let data3 = vec![7,8,9,10];
        conn1.send(data1.clone()).await.unwrap();
        conn1.send(data2.clone()).await.unwrap();
        conn1.send(data3.clone()).await.unwrap();
        assert_eq!(data1, conn2.recv().await.unwrap());
        assert_eq!(data2, conn2.recv().await.unwrap());
        assert_eq!(data3, conn2.recv().await.unwrap());
    }

    #[tokio::test]
    async fn test_large_payload() {
        let (mut conn1, mut conn2) = get_conns().await;
        let data: Vec<u8> = (1..655350).into_iter().map(|n: usize| (n % 256) as u8).collect();
        conn1.send(data.clone()).await.unwrap();
        assert_eq!(data, conn2.recv().await.unwrap());
    }
}
