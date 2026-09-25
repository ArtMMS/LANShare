# Changelog

## [0.2.0] - 2026-09-25

### Added
- Bidirectional communication between Host and Client.
    - The Host can now send messages to the Client while simultaneously receiving messages from the Client in real time.
- Added username identification for both Host and Client.

---

## [0.1.0] - 2026-09-25

### Added
- Host application: starts a TCP socket server and waits for a Client connection on port 5555.
- Client application: allows the user to enter the Host's IP address to connect to it.
- Connection establishment between Host and Client using socket (Python's built-in networking library).
- Basic message exchange from Client to Host.
- Real-time disconnection detection:
    - The Host immediately detects when the Client disconnects.
    - The Client immediately detects when the Host disconnects, using a dedicated thread for continuous listening.
- Communication implemented using raw socket without external dependencies.
- threading used on the Client to listen for messages from the Host while simultaneously handling user input.