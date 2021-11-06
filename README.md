# UDPP

> One more than UDP

An _experimental_ protocol adding the following on top of UDP:
- [x] Sessions
- [ ] Congestion detection
- [ ] Guaranteed order
- [ ] Handle larger packet sizes
- [ ] Authentication and encryption

Intended use case is general real-time data transfer where guaranteed delivery is not necessary.

## (Intended) Behavior
The following are just some rough ideas for how the implementation will work.

### Sessions
The client chooses a random 256-bit session identifier.
No handshake is necessary. The server creates a new `UdppStream` upon seeing a new session identifier.

### Congestion detection
Each packet contains a sequence number.
We track packet loss by counting the proportion of missing packets within a recent window.

### Guaranteed order
Packets that are older than a configurable window are ignored.

### Handle larger packet sizes
Large payloads are split over multiple UDP packets and re-assembled.
However, this increases the risk that the payload will be dropped.
This is not too bad because the congestion detection would report this problem and the sender should back-off in response.

### Authentication and encryption
In authenticated mode, the session identifier is a public key.
Packets lacking a valid signature by the public key would be ignored.

## Implementation

### Handshake
1. Client sends `ClientKeyAnnounce`
2. Server sends `ServerChallenge(ServerPublicKey, Hash(ClientPublicKey, TimeWindow, ServerHashSecret))`
3. Client sends `ClientHandshake(Signed(Hash(ClientPublicKey, TimeWindow, ServerHashSecret)), SessionKey)`
4. Server sends `ServerHandshake(EncryptedForClient(SessionKey))` // or maybe just go ahead and start sending stuff

Now both the client and the server have `SessionKey`, so communication can continue where each message is AEAD with this key.
Invalid packets are ignored.

Packets have the structure:
```rust
pub struct EncryptedUdppPacket {
    pub session_id: SessionId, // public key of peer, which maps to a SessionKey
    pub encrypted_payload: Vec<u8>,
}
```

The `encrypted_payload` is AEAD with the `SessionKey` and is structured as:
```rust
pub struct UdppPayload {
    pub index: u64,
    pub content: UdppContent,
}
```

`UdppContent` is an enum and can consist of many different types:
```rust
pub enum UdppContent {
    CongestionInformation(u64, u64, u64),
    CongestionAcknowledgement(u64),
    DataFragment(u64, u64, Vec<u8>), // fragment index, packet length, payload
}
```


At intervals, the client and server send `CongestionInformation(StartIndex, EndIndex, PacketsAccepted)`.
`StartIndex` should be index after the previous `EndIndex`.
`EndIndex` should be the index of the most recently received packet.
`PacketsAccepted` is the number of packets accepted in this range.

Immediately after receiving a `CongestionInformation`, the other side should send a `CongestionAcknowledgement(CongestionInformationIndex)`.
This allows the sender of the `CongestionInformation` to estimate latency.

