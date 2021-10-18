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
