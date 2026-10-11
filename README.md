# LANShare

![LANShare Logo](assets/logo.png)

![Status](https://img.shields.io/badge/status-in%20development-yellow)
![Version](https://img.shields.io/badge/version-v0.4-orange)
![Progress](https://img.shields.io/badge/progress-39%25-yellowgreen)
[![License](https://img.shields.io/badge/license-MIT-brightgreen)](LICENSE)
[![Support LANShare](https://img.shields.io/badge/Support%20LANShare-0078D4?style=flat&logo=github&logoColor=white)](#-support-lanshare)

LANShare is a local network application designed to share a computer's screen with other devices in real time. It aims to provide low-latency screen streaming over a LAN connection, with support for multiple users, multiple monitors, and specific window sharing.

The project is being developed with performance, stability, and ease of use in mind, with future plans to include synchronized audio streaming and support for higher resolutions and frame rates.


![gif](assets/demo_gif_0_4.gif)

---

## 📦 Instalation Guide

<p align="center"> <a href="https://github.com/ArtMMS/LANShare/releases/download/v0.4.0/LANShare-v0.4.0.rar"> <img src="https://img.shields.io/badge/⬇%20Download-LANShare%20v0.4.0-0078D4?style=for-the-badge" alt="Download LANShare v0.4.0"> </a> </p>

- **Version:** v0.4.0
- **Release:** Early Development Build
- **Platform:** Windows

---

### How to Install and Run

1. Download the .rar archive using the link above.
2. Extract the archive to a folder of your choice.
3. Run lanshare.exe.

*Note: LANShare is currently in early development. Features, performance, and compatibility may change between releases. The graphical interface is not yet available.*

Looking for an older version? Check the [GitHub Releases](../../releases) page.

---

## 🛠️ How to use

**Prerequisites:**
> The host and client PCs must be connected to the same LAN (Local Area Network) for LANShare to work. If the PCs are on different networks, you can use a VPN application such as Hamachi, Radmin VPN, or another alternative to create a virtual LAN.

### Host (Screen Sharing)

1. Run lanshare.exe
2. In the terminal, type "*h*" to start LANShare in Host mode.
3. Set a password for the room, or press **Enter** to create a room without a password.
4. LANShare will display the available network addresses. Find the IP address belonging to the LAN shared with the client PC and send it to the client.
5. Type **help** in the host terminal to see the available commands.
6. Type **share** to start screen sharing.
7. Type **stop** to stop screen sharing.

### Client (Receiving the Screen)

1. Run lanshare.exe
2. In the terminal, type "*c*" to start LANShare in Client mode.
3. LANShare will search for available rooms on the network. If the host is reachable on the same LAN or virtual LAN, its room should be discoverable.
4. Enter the host's IP address when prompted and enter the room password if one was set by the host.
5. Once connected, the client can receive the host's screen when the host starts sharing.

---

## ⚙️ Technologies

- **Rust** — Core application, networking and system logic
- **C#** — Desktop graphical user interface
- **Avalonia UI** — Cross-platform desktop user interface framework

---

## 🐛 Known Issues

- If the host starts streaming its screen to a client, stops the stream, and then starts streaming again, the application crashes.

---

## 📋 License

LANShare is released under the **MIT License**.

You are free to use, copy, modify, merge, publish, distribute, sublicense, and sell copies of the software, as long as the copyright notice and license text are included.

See the [LICENSE](LICENSE) file for the complete license terms.

---

## ☕💙 Support LANShare

LANShare is an open-source and free-to-use project. If you find LANShare useful and would like to support its development, consider sponsoring the project.

Your support helps with development, maintenance, testing, and future improvements.

[![Buy Me a Coffee](https://img.shields.io/badge/☕-Buy%20Me%20a%20Coffee-FF5E5B?style=flat&logo=ko-fi&logoColor=white)](https://ko-fi.com/artmms)
[![GitHub Sponsors](https://img.shields.io/badge/Sponsor-LANShare-2563EB?logo=github&logoColor=white)](https://github.com/sponsors/ArtMMS)