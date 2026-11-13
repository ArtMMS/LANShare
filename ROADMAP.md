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

# V1.1.0 — Audio Streaming

 [x] Capture system audio
 [x] Select the audio output device (default: Windows default device)
 [x] Transmit audio to connected users
 [x] Synchronize audio with the streamed image
 [x] Individual transmission volume control for each user

 ---

 # V1.2.0 — Soundboard

- [x] Add sound effects (.mp3 / .wav / .ogg, up to 5 seconds) saved in %APPDATA%\LANShare\sfx
- [x] Soundboard button and panel (3 sounds per row)
- [x] Rename, remove and preview sounds locally
- [x] Individual sound effects volume control for each user
- [ ] Play sound effects for everyone in the room (Host and Clients)
- [ ] Anti-spam cooldown enforced by the Host

---

 # V1.2.1 — Soundboard fixes

- [x] One sound at a time: the cooldown lasts as long as the sound that was just played
- [x] Sound effects can be up to 10 seconds long

---

# V1.3.0 — Smart Audio Capture

- [x] Add audio toggle button (🔊) to the monitor/window picker (green = on, red + slash = off, tooltip after 1 s)
- [x] Window mode: capture audio only from the shared app (process tree)
- [x] Handle UWP/Store apps in window mode
- [x] List active audio sessions (apps playing sound)
- [x] Capture each allowed app separately (process loopback INCLUDE)
- [x] Mix all app captures into a single audio stream
- [x] Detect apps that start/stop playing sound during the stream
- [x] Monitor mode: capture the whole system except blocked apps
- [x] Add "Apps excluded from audio" list in Settings (call apps blocked by default)
- [x] Allow adding/removing apps from the excluded list
- [x] Update audio mode when using "Change stream" (monitor ↔ window)

---

# V2.0.0 — Multi-Streaming

- [x] Split capture/encode and decode/playback into shared crates (lanshare-stream / lanshare-viewer)
- [x] Protocol v12: stream announcements and watch tickets
- [x] Every user can share their screen (Host and Clients)
- [x] Streams go directly from the streamer to the viewers (no relay through the Host)
- [x] Viewers only receive the streams they chose to watch
- [x] Host coordinates streams: announce, watch tickets, revoke on kick/leave
- [x] Host can watch other users' streams (video, audio, volume, fullscreen)
- [x] Stream controls for Clients (resolution, FPS, monitor/window picker, audio toggle)
- [x] Screen-sharing indicator next to the user's name in the user list
- [x] Select a stream by clicking the user (the user you're watching has a green border)
- [x] "X started streaming" notification for any user
- [x] (FIX) Stream speeds up on a bad connection: steady playback on the viewer
    - Show each frame at its capture time plus a small fixed delay (~100 ms)
    - Use a local clock when the stream has no audio
    - When frames arrive late in a burst, skip to the newest one instead of fast-forwarding
- [ ] (FIX) Stream freezes on a bad connection: adaptive bitrate on the streamer
    - Measure every viewer's queue each second on the StreamServer
    - Lower the encoder bitrate when queues grow (with a minimum), raise it back slowly when stable
    - Follow the slowest viewer (one encoder for everyone)
    - Show "Unstable connection" in the info bar
- [ ] (FIX) Monitor turns off while watching: Windows power request (like browsers do)
    - Keep the display on and the PC awake while watching a stream
    - Keep the display on and the PC awake while streaming
    - Release the request when watching/streaming stops or when leaving the room

# V2.1.0 — Multi-View & Webcam

- [ ] Watch other streams while streaming
- [ ] Screen that shows all active streams
   - [ ] Adaptative/Customizable layout (change position and size)
- [ ] Display the webcam feed of all connected users (webcam option on the users [& show avatars for those who doesn't have webcam turn on])
   - Only cameras and avatars screen (like discord or skype); clickable on the user feed
   - Allow Webcam Options (like OBS Studio)
- [ ] Ban a user after 2 kicks (by IP, only for the current room)
    - Count kicks per IP on the Host
    - Refuse banned IPs before asking for the password ("You are banned from this room")
    - The ban list is cleared when the Host opens a new room

---

## Future Updates
 Optimize
 Support 1440p (Quad HD) and 4K
 Reach 60 FPS

 Chat
   - Allow upload files, images and videos and allow download

 Stream multiple monitors at the same time (only one user)
    - Show untill 3 monitors, one on top of the other, and focus by click in one of them (showing the other ones on the bottom)



 Allow multiples screen transmitions and notify who is showing the screen on the user feed
    - Screen that shows all screen transmission with a customazible layout (change position and size)
    - Show or hide webcam options
    - Customize all webcam positions and sizes (automatic is on bottom)
 Linux Port

