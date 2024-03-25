mod handler;
mod ip_discovery;
mod session;
pub mod snow_types;
mod transform;
mod transport;
pub mod veq;

#[cfg(test)]
mod tests {

    use tokio::join;
    use uuid::Uuid;

    use crate::{
        snow_types::SnowKeypair,
        veq::{VeqSessionAlias, VeqSocket},
    };

    async fn get_conns() -> (VeqSessionAlias, VeqSessionAlias) {
        let mut socket1 = VeqSocket::bind("0.0.0.0:0").await.unwrap();
        let mut socket2 = VeqSocket::bind("0.0.0.0:0").await.unwrap();

        let id = Uuid::new_v4();
        let info1 = socket1.connection_info();
        let info2 = socket2.connection_info();
        let (alias1, alias2) = join!(socket1.connect(id, info2), socket2.connect(id, info1),);
        (alias1.unwrap(), alias2.unwrap())
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

        let data = vec![0, 1, 2, 3];
        conn1.send(data.clone()).await.unwrap();
        let received = conn2.recv().await;
        assert_eq!(data, received.unwrap());

        let data2 = vec![5, 6, 7, 8];
        conn2.send(data2.clone()).await.unwrap();
        let received2 = conn1.recv().await;
        assert_eq!(data2, received2.unwrap());
    }

    #[tokio::test]
    async fn test_no_premature_drop() {
        let (conn1_temp, mut conn2) = get_conns().await;
        let mut conn1 = conn1_temp.clone();
        drop(conn1_temp);

        let data = vec![0, 1, 2, 3];
        conn1.send(data.clone()).await.unwrap();
        let received = conn2.recv().await;
        assert_eq!(data, received.unwrap());

        let data2 = vec![5, 6, 7, 8];
        conn2.send(data2.clone()).await.unwrap();
        let received2 = conn1.recv().await;
        assert_eq!(data2, received2.unwrap());

        drop(conn1);
        assert!(conn2.recv().await.is_err());
    }

    #[tokio::test]
    async fn test_queued_small() {
        let (mut conn1, mut conn2) = get_conns().await;
        let data1 = vec![0, 1, 2, 3];
        let data2 = vec![5, 4, 3, 2];
        let data3 = vec![7, 8, 9, 10];
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
        let data: Vec<u8> = (1..655350).map(|n: usize| (n % 256) as u8).collect();
        conn1.send(data.clone()).await.unwrap();
        assert_eq!(data, conn2.recv().await.unwrap());
    }

    #[tokio::test]
    async fn test_session_id_reuse() {
        let mut socket1 = VeqSocket::bind("0.0.0.0:0").await.unwrap();
        let mut socket2 = VeqSocket::bind("0.0.0.0:0").await.unwrap();

        let id = Uuid::new_v4();
        let info1 = socket1.connection_info();
        let info2 = socket2.connection_info();

        let (session1, session2) = join!(
            socket1.connect(id, info2.clone()),
            socket2.connect(id, info1.clone()),
        );
        let mut session1 = session1.unwrap();
        let mut session2 = session2.unwrap();

        session1.send(vec![0, 1, 2, 3]).await.unwrap();
        assert_eq!(vec![0, 1, 2, 3], session2.recv().await.unwrap());

        drop(session1);
        drop(session2);

        let (session1, session2) =
            join!(socket1.connect(id, info2), socket2.connect(id, info1),);

        let mut session1 = session1.unwrap();
        let mut session2 = session2.unwrap();

        session1.send(vec![4, 5, 6, 7]).await.unwrap();
        assert_eq!(vec![4, 5, 6, 7], session2.recv().await.unwrap());
    }

    #[tokio::test]
    async fn test_drop_session() {
        let (conn1, mut conn2) = get_conns().await;
        drop(conn1);
        assert!(conn2.recv().await.is_err());
    }

    #[tokio::test]
    async fn test_bind_with_key() {
        let keypair = SnowKeypair::new().unwrap();
        let socket1 = VeqSocket::bind_with_keypair("0.0.0.0:0", keypair.clone())
            .await
            .unwrap();
        let socket2 = VeqSocket::bind_with_keypair("0.0.0.0:0", keypair)
            .await
            .unwrap();

        assert_eq!(
            socket1.connection_info().public_key,
            socket2.connection_info().public_key
        );
    }
}
