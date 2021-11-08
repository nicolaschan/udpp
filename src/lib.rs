#![feature(map_first_last)]
#![feature(async_stream)]

pub mod proto;
pub mod transport;

#[cfg(test)]
mod tests {
    use std::{net::{SocketAddr, ToSocketAddrs}};

    use async_std::channel::unbounded;

    use crate::{proto::{socket::{UdppServer, UdppSession}}};

    fn socket_addr(string: &str) -> SocketAddr {
        string
            .to_socket_addrs()
            .unwrap()
            .into_iter()
            .next()
            .unwrap()
    }

    async fn setup_channel_test() -> (SocketAddr, UdppServer, UdppSession) {
        let addr = socket_addr("127.0.0.1:8080");
        let (sender1, receiver1) = unbounded();
        let (sender2, receiver2) = unbounded();

        let server = UdppServer::new(Box::new(sender1), Box::new(receiver2));
        let conn = UdppSession::new(Box::new(sender2), Box::new(receiver1), addr).await;

        (addr, server, conn)
    }

    // #[test]
    // fn handles_empty_data() {
    //     let (sender1, receiver1) = unbounded();
    //     let (sender2, receiver2) = unbounded();

    //     let mut handler1: UdppHandler = UdppHandler::new(Box::new(sender1));
    //     let mut handler2: UdppHandler = UdppHandler::new(Box::new(sender2));


    //     let addr = socket_addr("127.0.0.1:8080");
    //     handler1.establish_session(addr);

    //     let (_addr, received ) = receiver1.recv().unwrap();
    //     println!("{:?}", received);

    //     handler2.handle_incoming(addr, received);
    //     let (_addr, received2 ) = receiver2.recv().unwrap();
    //     println!("{:?}", received2);

    //     println!("{:?}", handler1.sessions);
    //     println!("{:?}", handler2.sessions);

    //     handler1.handle_incoming(addr, received2);
    //     println!("{:?}", handler1.sessions);
    //     println!("{:?}", handler2.sessions);
    // }

    #[tokio::test]
    async fn test_client_sends_to_server() {
        let (_addr, server, mut conn) = setup_channel_test().await;

        let data: Vec<u8> = vec![1,2,3,4];
        let expected = data.clone();
        let mut incoming = server.incoming();
        let mut server_conn = incoming.accept().await;

        conn.send(data).await;
        let actual = server_conn.recv().await;
        assert_eq!(expected, actual);
    }

    #[tokio::test]
    async fn test_server_sends_to_client() {
        let (_addr, server, mut conn) = setup_channel_test().await;

        let data: Vec<u8> = vec![1,2,3,4];
        let expected = data.clone();
        let mut incoming = server.incoming();
        let mut server_conn = incoming.accept().await;

        server_conn.send(data).await;
        let actual = conn.recv().await;
        assert_eq!(expected, actual);
    }

    #[tokio::test]
    async fn test_bidirectional() {
        let (_addr, server, mut conn) = setup_channel_test().await;

        let data: Vec<u8> = vec![1,2,3,4];
        let expected = data.clone();
        let data2: Vec<u8> = vec![1,2,3,4];
        let expected2 = data2.clone();
        let mut incoming = server.incoming();
        let mut server_conn = incoming.accept().await;

        server_conn.send(data).await;
        conn.send(data2).await;

        let actual = conn.recv().await;
        assert_eq!(expected, actual);

        let actual2 = server_conn.recv().await;
        assert_eq!(expected2, actual2);

    }
}