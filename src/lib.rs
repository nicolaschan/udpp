#![feature(adt_const_params)]
#![feature(map_first_last)]
#![feature(async_stream)]
#![feature(untagged_unions)]

mod holepunch;
mod proto;
mod transport;

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_handshake() {
        // let mut initiator1 = SessionInitiator::new();
        // let mut initiator2 = SessionInitiator::new();

        // let initiation = initiator1.initiation();
        // let (session2, response) = initiator2.receive_initiation(initiation);

        // let session1 = initiator1.receive_response(response).unwrap();
    }
}
