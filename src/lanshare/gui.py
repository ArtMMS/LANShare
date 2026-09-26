from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import (
    QMainWindow, QWidget, QLabel, QTextEdit, QLineEdit,
    QPushButton, QVBoxLayout, QHBoxLayout, QFrame, QSlider
)

from chat_threads import ChatServerThread, ChatClientThread
from video_threads import VideoSendServerThread, VideoReceiveThread


DARK_STYLESHEET = """
QMainWindow, QWidget { background-color: #1b1f2b; color: #e6e8ef; font-family: 'Segoe UI', sans-serif; }
QFrame#sidebar { background-color: #20242f; border-right: 1px solid #2c313f; }
QLabel#appTitle { font-size: 16px; font-weight: 600; color: #ffffff; }
QLabel#sectionLabel { font-size: 11px; font-weight: 600; color: #8b93a7; margin-top: 10px; }
QLabel#statusText { font-size: 13px; color: #ffffff; }
QLabel#dimText { color: #8b93a7; font-size: 12px; }
QFrame#statusDot { background-color: #35d07f; border-radius: 6px; min-width: 12px; max-width: 12px; min-height: 12px; max-height: 12px; }
QFrame#statusDotOff { background-color: #6b7280; border-radius: 6px; min-width: 12px; max-width: 12px; min-height: 12px; max-height: 12px; }
QPushButton { background-color: #2c313f; color: #e6e8ef; border-radius: 6px; padding: 6px 12px; }
QPushButton:hover { background-color: #384056; }
QPushButton:disabled { color: #565d70; }
QPushButton#startButton { background-color: #2f7a4f; }
QPushButton#startButton:hover { background-color: #368a59; }
QPushButton#stopButton { background-color: #7a3030; }
QPushButton#stopButton:hover { background-color: #8a3838; }
QTextEdit, QLineEdit { background-color: #262b38; border: 1px solid #343b4c; border-radius: 6px; color: #e6e8ef; }
QLabel#liveBadge {
    background-color: rgba(30, 95, 200, 220);
    color: white; font-weight: 600; font-size: 11px;
    border-radius: 10px; padding: 4px 10px;
}
QLabel#videoPlaceholder { color: #8b93a7; font-size: 14px; }
"""


def status_dot(active):
    dot = QFrame()
    dot.setObjectName("statusDot" if active else "statusDotOff")
    return dot


class VideoContainer(QWidget):
    """Área de vídeo com o badge 'LANShare Live' flutuando por cima, no canto superior direito."""

    def __init__(self):
        super().__init__()
        self.video_label = QLabel("Aguardando stream...")
        self.video_label.setObjectName("videoPlaceholder")
        self.video_label.setAlignment(Qt.AlignCenter)
        self.video_label.setScaledContents(True)
        self.video_label.setParent(self)
        self.video_label.setStyleSheet("background-color: #0f1218;")

        self.badge = QLabel("🔷 LANShare Live")
        self.badge.setObjectName("liveBadge")
        self.badge.setParent(self)
        self.badge.adjustSize()
        self.badge.hide()

    def set_live(self, live):
        self.badge.setVisible(live)

    def update_frame(self, frame_bytes):
        image = QImage.fromData(frame_bytes, "JPG")
        pixmap = QPixmap.fromImage(image)
        self.video_label.setPixmap(pixmap)

    def resizeEvent(self, event):
        self.video_label.setGeometry(0, 0, self.width(), self.height())
        margin = 14
        self.badge.move(self.width() - self.badge.width() - margin, margin)
        super().resizeEvent(event)


class ChatPanel(QWidget):
    """Painel de chat reutilizável, usado tanto pelo Host quanto pelo Client."""

    def __init__(self, my_username, chat_thread):
        super().__init__()
        self.my_username = my_username
        self.chat_thread = chat_thread

        self.chat_log = QTextEdit()
        self.chat_log.setReadOnly(True)
        self.chat_log.setMaximumHeight(160)

        self.input_line = QLineEdit()
        self.input_line.setPlaceholderText("Mensagem...")
        self.input_line.returnPressed.connect(self._on_send)

        self.send_button = QPushButton("Enviar")
        self.send_button.clicked.connect(self._on_send)

        self.leave_button = QPushButton("Sair da rede")
        self.leave_button.clicked.connect(self._on_leave)

        input_row = QHBoxLayout()
        input_row.addWidget(self.input_line)
        input_row.addWidget(self.send_button)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.chat_log)
        layout.addLayout(input_row)
        layout.addWidget(self.leave_button)

        self.chat_thread.peer_connected.connect(self._on_peer_connected)
        self.chat_thread.message_received.connect(self._on_message_received)
        self.chat_thread.disconnected.connect(self._on_disconnected)

        self.on_peer_connected_callback = None
        self.on_disconnected_callback = None

        self.heartbeat_timer = QTimer(self)
        self.heartbeat_timer.timeout.connect(self.chat_thread.send_ping)
        self.heartbeat_timer.start(5000)

        self.watchdog_timer = QTimer(self)
        self.watchdog_timer.timeout.connect(self.chat_thread.check_timeout)
        self.watchdog_timer.start(1000)

        self.chat_thread.start()

    def _on_peer_connected(self, peer_username):
        if self.on_peer_connected_callback:
            self.on_peer_connected_callback(peer_username)

    def _on_message_received(self, username, text):
        self.chat_log.append(f"<b>{username}:</b> {text}")

    def _on_send(self):
        text = self.input_line.text().strip()
        if not text:
            return
        self.chat_thread.send_message(text)
        self.chat_log.append(f"<b>{self.my_username}:</b> {text}")
        self.input_line.clear()

    def _on_leave(self):
        self.chat_thread.send_disconnect_signal()
        self.chat_thread.stop()

    def _on_disconnected(self, reason):
        self.heartbeat_timer.stop()
        self.watchdog_timer.stop()
        if self.on_disconnected_callback:
            self.on_disconnected_callback(reason)

    def shutdown(self):
        self.heartbeat_timer.stop()
        self.watchdog_timer.stop()
        self.chat_thread.stop()
        self.chat_thread.wait()


class Sidebar(QFrame):
    def __init__(self, username, role_label):
        super().__init__()
        self.setObjectName("sidebar")
        self.setFixedWidth(280)

        self.layout = QVBoxLayout(self)
        self.layout.setContentsMargins(16, 16, 16, 16)
        self.layout.setSpacing(4)

        title = QLabel("LANShare <span style='color:#8b93a7; font-size:11px;'>v1.0</span>")
        title.setObjectName("appTitle")
        self.layout.addWidget(title)
        self.layout.addSpacing(10)

        # Status
        self.layout.addWidget(self._section_label("STATUS"))
        status_row = QHBoxLayout()
        self.status_dot = status_dot(False)
        self.status_text = QLabel(role_label)
        self.status_text.setObjectName("statusText")
        status_row.addWidget(self.status_dot)
        status_row.addWidget(self.status_text)
        status_row.addStretch()
        self.layout.addLayout(status_row)

        # Conexão
        self.layout.addWidget(self._section_label("CONEXÃO"))
        self.connection_label = QLabel("—")
        self.connection_label.setObjectName("dimText")
        self.layout.addWidget(self.connection_label)

        # Usuários
        self.layout.addWidget(self._section_label("USUÁRIOS"))
        self.users_layout = QVBoxLayout()
        self.layout.addLayout(self.users_layout)
        self.add_user(f"{username} (Você)")

        # Controles de Áudio (V0.5 - desabilitado)
        self.layout.addWidget(self._section_label("CONTROLES DE ÁUDIO (V0.5)"))
        audio_row = QHBoxLayout()
        audio_slider = QSlider(Qt.Horizontal)
        audio_slider.setEnabled(False)
        mute_button = QPushButton("Mute")
        mute_button.setEnabled(False)
        audio_row.addWidget(audio_slider)
        audio_row.addWidget(mute_button)
        self.layout.addLayout(audio_row)

        # Stream Info
        self.layout.addWidget(self._section_label("INFO DO STREAM"))
        self.fps_label = QLabel("FPS: —")
        self.fps_label.setObjectName("dimText")
        self.latency_label = QLabel("Latência: —")
        self.latency_label.setObjectName("dimText")
        self.bitrate_label = QLabel("Bitrate: —")
        self.bitrate_label.setObjectName("dimText")
        self.layout.addWidget(self.fps_label)
        self.layout.addWidget(self.latency_label)
        self.layout.addWidget(self.bitrate_label)

        self.layout.addStretch()

        # Chat (colapsável)
        self.chat_toggle = QPushButton("💬 Mostrar chat")
        self.layout.addWidget(self.chat_toggle)
        self.chat_container = QWidget()
        self.chat_container.setVisible(False)
        self.chat_container_layout = QVBoxLayout(self.chat_container)
        self.chat_container_layout.setContentsMargins(0, 8, 0, 0)
        self.layout.addWidget(self.chat_container)
        self.chat_toggle.clicked.connect(self._toggle_chat)

        # Botões de controle (preenchidos por quem instancia, se necessário)
        self.controls_layout = QHBoxLayout()
        self.layout.addLayout(self.controls_layout)

    def _section_label(self, text):
        label = QLabel(text)
        label.setObjectName("sectionLabel")
        return label

    def _toggle_chat(self):
        visible = not self.chat_container.isVisible()
        self.chat_container.setVisible(visible)
        self.chat_toggle.setText("💬 Ocultar chat" if visible else "💬 Mostrar chat")

    def set_chat_panel(self, chat_panel):
        self.chat_container_layout.addWidget(chat_panel)

    def add_user(self, label_text):
        entry = QLabel(f"👤 {label_text}")
        entry.setObjectName("statusText")
        self.users_layout.addWidget(entry)

    def set_status(self, active, text):
        self.status_dot.setObjectName("statusDot" if active else "statusDotOff")
        self.status_dot.setStyleSheet(DARK_STYLESHEET)  # força reavaliação do objectName
        self.status_text.setText(text)

    def set_connection(self, text):
        self.connection_label.setText(text)

    def set_stream_info(self, fps=None, latency_ms=None, bitrate_kbps=None):
        if fps is not None:
            self.fps_label.setText(f"FPS: {fps:.0f}")
        if latency_ms is not None:
            self.latency_label.setText(f"Latência: {latency_ms:.0f}ms")
        if bitrate_kbps is not None:
            self.bitrate_label.setText(f"Bitrate: {bitrate_kbps:.0f} kbps")


class HostWindow(QMainWindow):
    def __init__(self, username, monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors,
                 resolution_scale=1.0, target_bitrate_kbps=4000):
        super().__init__()
        self.setWindowTitle("LANShare - Host")
        self.resize(1200, 700)
        self.setStyleSheet(DARK_STYLESHEET)

        self.sidebar = Sidebar(username, "Host (Conectando...)")
        self.sidebar.set_connection("Aguardando Client...")

        self.video_container = VideoContainer()

        self.start_button = QPushButton("▶ Start Streaming")
        self.start_button.setObjectName("startButton")
        self.stop_button = QPushButton("⏹ Stop Streaming")
        self.stop_button.setObjectName("stopButton")
        self.stop_button.setEnabled(False)
        self.start_button.clicked.connect(self._on_start)
        self.stop_button.clicked.connect(self._on_stop)
        self.sidebar.controls_layout.addWidget(self.start_button)
        self.sidebar.controls_layout.addWidget(self.stop_button)

        chat_thread = ChatServerThread(username)
        self.chat_panel = ChatPanel(username, chat_thread)
        self.chat_panel.on_peer_connected_callback = self._on_peer_connected
        self.chat_panel.on_disconnected_callback = self._on_peer_disconnected
        self.sidebar.set_chat_panel(self.chat_panel)

        main_layout = QHBoxLayout()
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.addWidget(self.sidebar)
        main_layout.addWidget(self.video_container)

        container = QWidget()
        container.setLayout(main_layout)
        self.setCentralWidget(container)

        self.video_thread = VideoSendServerThread(
            monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors,
            resolution_scale=resolution_scale, target_bitrate_kbps=target_bitrate_kbps
        )
        self.video_thread.client_connected.connect(self._on_client_connected)
        self.video_thread.fps_updated.connect(lambda fps: self.sidebar.set_stream_info(fps=fps))
        self.video_thread.stats_updated.connect(
            lambda kbps, quality: self.sidebar.set_stream_info(bitrate_kbps=kbps)
        )
        self.video_thread.streaming_state_changed.connect(self._on_streaming_state_changed)
        self.video_thread.error_occurred.connect(
            lambda msg: self.sidebar.set_status(False, f"Erro: {msg}")
        )
        self.video_thread.start()

    def _on_client_connected(self):
        self.sidebar.set_status(True, "Host (Pronto)")
        self.sidebar.set_connection("Client conectado")
        self.start_button.setEnabled(True)

    def _on_peer_connected(self, peer_username):
        self.sidebar.add_user(f"{peer_username} (Client)")

    def _on_peer_disconnected(self, reason):
        self.sidebar.set_status(False, f"Host ({reason})")

    def _on_start(self):
        self.video_thread.start_streaming()

    def _on_stop(self):
        self.video_thread.stop_streaming()

    def _on_streaming_state_changed(self, active):
        self.start_button.setEnabled(not active)
        self.stop_button.setEnabled(active)
        self.video_container.set_live(active)
        if active:
            self.sidebar.set_status(True, "Host (Broadcasting)")
        else:
            self.sidebar.set_status(True, "Host (Pronto)")

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        self.video_thread.stop()
        self.video_thread.wait()
        event.accept()


class ClientWindow(QMainWindow):
    def __init__(self, host_ip, username):
        super().__init__()
        self.setWindowTitle("LANShare - Client")
        self.resize(1200, 700)
        self.setStyleSheet(DARK_STYLESHEET)

        self.sidebar = Sidebar(username, "Client (Conectando...)")
        self.sidebar.set_connection(host_ip)

        self.video_container = VideoContainer()

        chat_thread = ChatClientThread(username, host_ip)
        self.chat_panel = ChatPanel(username, chat_thread)
        self.chat_panel.on_peer_connected_callback = self._on_peer_connected
        self.chat_panel.on_disconnected_callback = self._on_peer_disconnected
        self.sidebar.set_chat_panel(self.chat_panel)

        main_layout = QHBoxLayout()
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.addWidget(self.sidebar)
        main_layout.addWidget(self.video_container)

        container = QWidget()
        container.setLayout(main_layout)
        self.setCentralWidget(container)

        self._frame_count = 0
        self._bytes_count = 0
        self._last_report_time = None

        self.video_thread = VideoReceiveThread(host_ip)
        self.video_thread.frame_received.connect(self._on_frame_received)
        self.video_thread.connection_lost.connect(self._on_connection_lost)
        self.video_thread.start()

    def _on_peer_connected(self, peer_username):
        self.sidebar.set_status(True, "Client (Viewing)")
        self.sidebar.add_user(f"{peer_username} (Host)")
        self.video_container.set_live(True)

    def _on_peer_disconnected(self, reason):
        self.sidebar.set_status(False, f"Client ({reason})")
        self.video_container.set_live(False)

    def _on_frame_received(self, frame_bytes, latency):
        import time
        self.video_container.update_frame(frame_bytes)

        self._frame_count += 1
        self._bytes_count += len(frame_bytes)
        now = time.time()
        if self._last_report_time is None:
            self._last_report_time = now
        elapsed = now - self._last_report_time
        if elapsed >= 1.0:
            fps = self._frame_count / elapsed
            kbps = (self._bytes_count * 8 / 1024) / elapsed
            self.sidebar.set_stream_info(fps=fps, latency_ms=latency * 1000, bitrate_kbps=kbps)
            self._frame_count = 0
            self._bytes_count = 0
            self._last_report_time = now

    def _on_connection_lost(self):
        self.sidebar.set_status(False, "Client (Desconectado)")
        self.video_container.set_live(False)

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        self.video_thread.stop()
        self.video_thread.wait()
        event.accept()