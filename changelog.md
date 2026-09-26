# Changelog

## [0.4.0] - 2026-09-26

### Added

* Unified application (`main.py` + `gui.py`): chat and video now run in a single session/window, with an initial **Launcher** screen to choose between "Share my screen" or "Join a stream".
* Monitor selection for screen sharing, now with a **visual preview** of each detected monitor.
* Specific application window selection instead of sharing the entire monitor, with automatic region cropping and support for moving the window between monitors during the stream.
* Host can now **stream without a Client connected** — capture starts as soon as streaming begins, and Clients can join or leave at any time without interrupting the Host.
* Client can **leave and rejoin** a stream at any time without restarting the application.
* Host-side **self-preview** of the active stream, replacing the empty waiting screen while streaming.
* Functional **Start/Stop Streaming** buttons: Stop fully terminates capture and returns to the "Waiting for stream" state; Start reopens the complete configuration (monitor, window, resolution, FPS, bitrate) each time.
* Graphical interface with a sidebar (status, connection, users, reserved audio controls for V0.5, stream information) and an integrated, collapsible chat panel.
* Live interface metrics: FPS, bitrate, and **real latency** (calculated using a timestamp embedded in each frame).
* Configurable **stream resolution** (720p, 1080p, 1440p, 2160p) and **frame rate** (15/30/60 FPS), selectable before starting the stream.
* **Dynamic bitrate control**: JPEG quality is automatically adjusted to keep the actual bitrate close to the selected target (Low/Medium/High).
* Automatic mapping between monitors (MSS) and capture outputs (DXCam) based on visual similarity, eliminating the need for manual calibration.
* Synthetic cursor rendered on top of the frame.
* Old standalone scripts (`host.py`, `client.py`, `stream_host.py`, `stream_client_gui.py`), superseded by the unified application.
* Manual monitor calibration workflow (replaced by automatic mapping).

### Technical

* Added new external dependencies: `dxcam`, `pywin32`.
* Updated the framing protocol (`streaming.py`) to include a capture timestamp in each frame, enabling latency calculation.

---

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