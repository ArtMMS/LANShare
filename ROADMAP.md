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
- [x] Display the streamed screen on the Client
- [x] Reduce streaming latency
- [x] Optimize bandwidth usage
- [x] Selectable resolution (720p / 1080p) and FPS (15 / 30)

---

# V0.4 — Graphical User Interface & Screen Selection

- [x] Create a single `.exe`
- [x] Create Host and Client GUIs
- [x] Display application version
- [x] Display connection status
- [x] Display connected users
- [x] Broadcast preview for the host
- [x] Ask for a user name on first launch and save it
- [x] Ask for the password when joining a protected room
- [x] Add Host options: kick clients and set a password
- [x] Add a screen sharing button
- [x] Detect multiple monitors
- [x] Display monitor names and previews
- [x] Select and stream a specific monitor
- [x] Detect available windows
- [x] Display window names, application icons and previews
- [x] Select and stream a specific window
- [x] Follow the shared window when it moves, resizes or changes monitor
- [x] Add screen sharing controls and start/stop buttons
- [x] Add resolution and FPS selectors (720p / 1080p, 15 / 30 FPS)
- [x] Add fullscreen mode (F11)

---

# V0.5 — Automatic Updates

- [x] Check for new versions on GitHub
- [x] Compare installed version with the latest version
- [x] Detect available updates automatically
- [x] Notify the user when an update is available
- [x] Add an update/install notification
- [x] Download the new version
- [x] Install the update
- [x] Restart the application after updating

---

# Future Plans

## Audio Streaming

 [x] Capture system audio
 [x] Select the audio output device (default: Windows default device)
 [x] Transmit audio to connected users
 [ ] Synchronize audio with the streamed image
 [ ] Individual transmission volume control for each user

---

## Future Updates
 Optimize
 Support 1440p (Quad HD)
 Support 4K
 Reach 60 FPS
 Stream multiple monitors at the same time as one large screen
 Display the webcam feed of all connected users (webcam option on the users [& show avatars for those who doesn't have webcam turn on])
 Allow multiples screen transmitions and notify who is showing the screen on the user feed
 Linux Port