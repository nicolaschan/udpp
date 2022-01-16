use async_trait::async_trait;

#[async_trait]
pub trait Bidirectional {
    async fn send(&mut self, data: Vec<u8>);
    async fn recv(&mut self) -> Vec<u8>;
}