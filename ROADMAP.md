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
- [ ] Detect multiple monitors
- [ ] Display monitor names and previews
- [ ] Select and stream a specific monitor
- [ ] Detect available windows
- [ ] Display window names, application icons and previews
- [ ] Select and stream a specific window
- [ ] Follow the shared window when it moves, resizes or changes monitor
- [x] Add screen sharing controls and start/stop buttons
- [x] Add resolution and FPS selectors (720p / 1080p, 15 / 30 FPS)
- [x] Add fullscreen mode (F11)

---

# V0.5 — Automatic Updates

- [ ] Check for new versions on GitHub
- [ ] Compare installed version with the latest version
- [ ] Detect available updates automatically
- [ ] Notify the user when an update is available
- [ ] Add an update/install notification
- [ ] Download the new version
- [ ] Install the update
- [ ] Restart the application after updating

---

# Distribution & Release

## Windows

- [ ] Create Windows Installer (`LANShare-Setup.exe`)
- [ ] Create portable version (`LANShare-win-x64.zip`)
- [ ] Test clean installation on Windows
- [ ] Test portable version on a clean Windows installation
- [ ] Configure application metadata and version information
- [ ] Create Start Menu shortcut
- [ ] Add application uninstaller
- [ ] Code-sign the application
- [ ] Code-sign the installer
- [ ] Verify Windows SmartScreen reputation

## Website

- [ ] Create LANShare download website
- [ ] Add Windows Installer download
- [ ] Add Portable version download
- [ ] Display current version
- [ ] Add system requirements
- [ ] Add installation instructions
- [ ] Add changelog / release notes
- [ ] Add GitHub repository link
- [ ] Add discreet advertising

## Releases

- [ ] Define release process
- [ ] Create versioned releases on GitHub
- [ ] Upload installer to GitHub Releases
- [ ] Upload portable version to GitHub Releases
- [ ] Update website download links
- [ ] Publish release notes

---

# Future Plans

## Audio Streaming

 Capture system audio
 Select the audio output device (default: Windows default device)
 Transmit audio to connected users
 Synchronize audio with the streamed image
 Individual transmission volume control for each user

---

## High-Performance Streaming
 Optimize
 Support 1440p (Quad HD)
 Support 4K
 Reach 60 FPS
 Stream multiple monitors at the same time as one large screen
 Display the webcam feed of all connected users
 Linux Port