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

# V2.0.0 — Multi-Streaming & Webcam

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
- [x] (FIX) Stream freezes on a bad connection: adaptive bitrate on the streamer
    - Measure every viewer's queue each second on the StreamServer
    - Lower the encoder bitrate when queues grow (with a minimum), raise it back slowly when stable
    - Follow the slowest viewer (one encoder for everyone)
    - Show "Unstable connection" in the info bar
- [x] (FIX) Monitor turns off while watching: Windows power request (like browsers do)
    - Keep the display on and the PC awake while watching a stream
    - Keep the display on and the PC awake while streaming
    - Release the request when watching/streaming stops or when leaving the room
- [x] Watch other streams while streaming

## Room view (default screen)

- [x] Room screen with every user in a grid (like Discord / Google Meet)
    - Grid adapts to the number of users (1, 2, 3–4, 5–6, 7–9...)
    - Each tile shows the camera (when on) or the avatar, the name and a screen-sharing badge
    - The tile of whoever is playing a sound effect lights up (like the user list)
- [x] It is the default screen of the room (Host and Client)
    - Watching a stream replaces it; closing the stream goes back to it
    - Clicking "USERS IN THE ROOM" opens it
    - Clicking someone's avatar in the user list opens it (streaming or not)
- [x] Bottom bar of the room screen
    - Soundboard button
    - Camera button: white = off, green = on
    - Share screen button (monitor icon): white = off, green = on; opens the source picker
- [x] Move the resolution/FPS selectors into the source picker (next to the audio button)

## Camera

- [x] Capture the webcam (Media Foundation)
- [x] Protocol: each stream has a kind (screen or camera); a user can have both at the same time
- [x] Camera goes to everyone in the room automatically, in two qualities
    - 720p for the tiles (room view and camera overlay)
    - Full quality (up to 4K / 60 FPS) only for whoever clicks to watch that camera in big
- [x] Camera icon next to the user's name in the user list (next to the screen icon)
- [x] Settings → Camera
    - Device selector, with a live preview
    - Resolution (up to 4K) and FPS (up to 60), listing only what the device supports

## Camera overlay (while watching a stream)

- [x] Floating 720p widgets with the cameras that are on, over the stream (no avatars), even in fullscreen (F11)
- [x] Each widget can be moved anywhere on the screen and resized
- [ ] Right click on a widget hides that camera
   - [ ] Click on the camera icon on the user feed also hides that camera. Click again shows it up again (make the icon goes red with a risk in the middle)
- [ ] Shortcut (default C, configurable in Settings → Shortcuts)
    - Press once: hides all cameras
    - Press again: shows all cameras back, including the ones hidden one by one
- [ ] Remember the position and size of each camera between streams

- [ ] Unlock 60 FPS Screen Transmission
   - Se der problema na entrega para o client, a taxa de FPS (APENAS DESTE CLIENT) irá diminuir automaticamente para 30 ou 15 FPS. Para evitar lag
---

## Future Updates

 Chat
   - Allow upload files, images and videos and allow download
   - Commands (Admin only) in chat [/ban /disban]

 Android Port (Kotlin + Jetpack Compose)
   - Allow only cameras transmission

 Stream multiple monitors at the same time (only one user)
    - Show untill 3 monitors, one on top of the other, and focus by click in one of them (showing the other ones on the bottom)



 Allow multiples screen transmitions and notify who is showing the screen on the user feed
    - Screen that shows all screen transmission with a customazible layout (change position and size)
    - Show or hide webcam options
    - Customize all webcam positions and sizes (automatic is on bottom)
 Linux Port

