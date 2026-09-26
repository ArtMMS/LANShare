# Changelog

## [0.3.0] - 2026-09-26

### Added

* Full-screen capture on the Host, with monitor selection when multiple displays are available.
* JPEG frame compression before transmission, significantly reducing the amount of data sent over the network.
* Mouse cursor rendered manually on top of the frame, as it is not included in the original screen capture.
* Continuous frame transmission from the Host to the Client through a dedicated socket (port `5556`), using a framing protocol (size prefix) to delimit frames over the TCP stream.
* Real-time stream display in a PySide6 window on the Client, using a dedicated thread to receive frames without blocking the user interface.

### Performance

* Replaced Pillow with OpenCV for faster JPEG compression, eliminating unnecessary color conversion.
* Replaced MSS with DXCam (DXGI Desktop Duplication) for screen capture, eliminating the main performance bottleneck identified through profiling.
* ~30 FPS target achieved (~29.5 FPS average).

### Technical

* Added new external dependencies: `opencv-python`, `numpy`, `dxcam`, `PySide6`.
* Removed `Pillow` dependency (replaced by OpenCV).

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