# p2p-mls
Proof-of-concept P2P delivery service for OpenMLS, built on top of libp2p. 

Usage: 
```
cargo run // Starts a messenger node that listens on a local tcp port
node create // Start a group
cargo run // In another terminal, start a new messenger node
node join // Join the group (sends key package and first node will respond with a welcome message)
node send // Send a message
````
