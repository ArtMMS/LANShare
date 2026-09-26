# Changelog

## [0.3.0] - 2026-09-26

### Added
-

---

## [0.2.0] - 2026-09-25

### Added
- Bidirectional communication between Host and Client.
    - The Host can now send messages to the Client while simultaneously receiving messages from the Client in real time.
- Added username identification for both Host and Client.
- Added graceful disconnection handling using the __DISCONNECT__ control message.
- Host and Client now notify each other when a user intentionally leaves the network.
- Added a user-friendly exit command (sair) for both Host and Client.
- Added timeout-based detection for silent connection failures.

### Improved
- Chat messages sent by the local user are now displayed with their username.
- Improved chat readability by formatting local and remote messages consistently.
- Reduced terminal message duplication by clearing and rewriting the user's input line before displaying the formatted message.
- Added support for handling __DISCONNECT__ messages on both Host and Client.
- Improved synchronization between communication and shutdown routines.
- Enhanced terminal output behavior using ANSI escape sequences (\033[F\033[K) to prevent duplicated message display.
- Improved reliability of connection status detection.
- Connections are now automatically closed when no response is received within the configured timeout period.

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