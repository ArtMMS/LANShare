# LANShare Roadmap

# V0.1 — Foundation

- [x] Define project structure
- [x] Establish basic Host ↔ Client communication
- [x] Connect devices through LAN
- [x] Detect connection and disconnection
- [x] Keep the connection open after the handshake
- [x] Send Bye when a Host or Client exits cleanly
- [x] Measure ping (Ping/Pong round-trip time)

---

# V0.2 — Multi-User Support

- [x] Support multiple clients
- [x] Manage multiple connections
- [x] Identify connected users
- [x] Handle user connections and disconnections
- [x] Save the user name in a local settings file
- [x] Add optional Host password
- [x] Kick a connected client
- [x] Automatic room discovery on the LAN (UDP broadcast)
- [x] Keep manual connection by IP as a fallback

---

# V0.3 — Screen Streaming

- [x] Capture Host screen
- [x] Encode/compress frames
- [x] Transmit screen frames to Clients
- [] Display the streamed screen on the Client
- [x] Reduce streaming latency
- [] Optimize bandwidth usage
- [] Selectable resolution (720p / 1080p) and FPS (15 / 30)
- [] Option to show or hide the Host cursor in the stream
- [] Measure FPS and bitrate in real time

---

# V0.4 — Screen Selection

- [ ] Add screen sharing button
- [ ] Detect multiple monitors
- [ ] Select which monitor to share
- [ ] Display monitor names and previews
- [ ] Stream the selected monitor
- [ ] Detect available windows
 Show window names, application icons and previews
- [ ] Select a specific window
- [ ] Stream a specific window
Follow the shared window when it moves, resizes or changes monitor
Show the stream resolution and FPS in the room information

---

# V0.5 — Graphical User Interface

- [ ] Create single .exe
- [ ] Create Host GUI
- [ ] Create Client GUI
- [ ] Display application version
- [ ] Display connection status
- [ ] Display connected users
- [ ] Add screen sharing controls
- [ ] Add monitor selection interface
- [ ] Add window selection interface
- [ ] Add start/stop streaming controls
Ask for a user name on first launch and keep it saved
Add resolution and FPS selectors to the monitor/window selection screen
Add room list with name, IP, resolution/FPS and number of users
Ask for the password when joining a protected room
Add Host options: kick client and set password
Add Host performance monitor (FPS, bitrate, ping)
Show ping on the Client
Add fullscreen mode (F11)
Add option to show the Host cursor

---

# V0.6 — Automatic Updates

- [ ] Check for new versions on GitHub
- [ ] Compare installed version with the latest version
- [ ] Detect available updates automatically
- [ ] Notify the user when an update is available
- [ ] Add an update/install button
- [ ] Download the new version
- [ ] Install the update
- [ ] Restart the application after updating

---

# V1.0 — First Stable Release

- [ ] LAN connection
- [ ] 1080p / 30 FPS screen streaming
- [ ] Multiple users
- [ ] Host GUI
- [ ] Client GUI
- [ ] Application version display
- [ ] Multi-monitor selection
- [ ] Monitor preview
- [ ] Specific window streaming
 Follow the shared window
Selectable resolution and FPS
 Performance monitor (FPS, bitrate, ping)
 User name saved on first launch
 Host password and kick client
 Automatic room discovery
 Fullscreen mode and Host cursor option
- [ ] Automatic update detection
- [ ] Update installation through the application

---

# Future Plans

## Distribution
 Create a download website with discreet ads
 Sign the application to avoid Windows SmartScreen warnings

---

## Audio Streaming

 Capture system audio
 Select the audio output device (default: Windows default device)
 Transmit audio to connected users
 Synchronize audio with the streamed image
 Individual transmission volume control for each user

---

## High-Performance Streaming

 Support resolutions above 1080p
 Support 1440p (Quad HD)
 Support 4K
 Reach 60 FPS
 Stream multiple monitors at the same time as one large screen
 Improve streaming quality
 Further reduce latency
 Optimize bandwidth usage

---

# Long-Term Goals

- [ ] Improve overall performance
- [ ] Improve streaming stability
- [ ] Improve user experience
- [ ] Expand streaming configuration options
- [ ] Continue improving the application beyond V1.0
