pub trait MaybeDual {
    type SocketT;

    fn v4(&self) -> Option<&Self::SocketT>;
    fn v6(&self) -> Option<&Self::SocketT>;
}
