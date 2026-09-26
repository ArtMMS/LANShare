from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import (
    QMainWindow, QWidget, QLabel, QTextEdit, QLineEdit,
    QPushButton, QVBoxLayout, QHBoxLayout, QSplitter
)

from chat_threads import ChatServerThread, ChatClientThread
from video_threads import VideoSendServerThread, VideoReceiveThread


class ChatPanel(QWidget):
    """Painel de chat reutilizável, usado tanto pelo Host quanto pelo Client."""

    def __init__(self, my_username, chat_thread):
        super().__init__()
        self.my_username = my_username
        self.chat_thread = chat_thread

        self.status_label = QLabel("Conectando...")
        self.chat_log = QTextEdit()
        self.chat_log.setReadOnly(True)

        self.input_line = QLineEdit()
        self.input_line.setPlaceholderText("Digite uma mensagem e aperte Enter...")
        self.input_line.returnPressed.connect(self._on_send)

        self.send_button = QPushButton("Enviar")
        self.send_button.clicked.connect(self._on_send)

        self.leave_button = QPushButton("Sair da rede")
        self.leave_button.clicked.connect(self._on_leave)

        input_row = QHBoxLayout()
        input_row.addWidget(self.input_line)
        input_row.addWidget(self.send_button)

        layout = QVBoxLayout(self)
        layout.addWidget(self.status_label)
        layout.addWidget(self.chat_log)
        layout.addLayout(input_row)
        layout.addWidget(self.leave_button)

        self.chat_thread.status_changed.connect(self.status_label.setText)
        self.chat_thread.peer_connected.connect(self._on_peer_connected)
        self.chat_thread.message_received.connect(self._on_message_received)
        self.chat_thread.disconnected.connect(self._on_disconnected)

        # heartbeat: envia sinal de vida a cada 5s e confere a cada 1s se o outro lado sumiu
        self.heartbeat_timer = QTimer(self)
        self.heartbeat_timer.timeout.connect(self.chat_thread.send_ping)
        self.heartbeat_timer.start(5000)

        self.watchdog_timer = QTimer(self)
        self.watchdog_timer.timeout.connect(self.chat_thread.check_timeout)
        self.watchdog_timer.start(1000)

        self.chat_thread.start()

    def _on_peer_connected(self, peer_username):
        self.status_label.setText(f"Conectado com: {peer_username}")

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
        self.status_label.setText("Você saiu da rede.")

    def _on_disconnected(self, reason):
        self.status_label.setText(f"[STATUS] {reason}")
        self.heartbeat_timer.stop()
        self.watchdog_timer.stop()

    def shutdown(self):
        self.heartbeat_timer.stop()
        self.watchdog_timer.stop()
        self.chat_thread.stop()
        self.chat_thread.wait()


class HostWindow(QMainWindow):
    def __init__(self, username, monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors):
        super().__init__()
        self.setWindowTitle("LANShare - Host")
        self.resize(700, 500)

        self.video_status_label = QLabel("Aguardando conexão de vídeo...")

        chat_thread = ChatServerThread(username)
        self.chat_panel = ChatPanel(username, chat_thread)

        layout = QVBoxLayout()
        layout.addWidget(self.video_status_label)
        layout.addWidget(self.chat_panel)

        container = QWidget()
        container.setLayout(layout)
        self.setCentralWidget(container)

        self.video_thread = VideoSendServerThread(
            monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors
        )
        self.video_thread.client_connected.connect(
            lambda: self.video_status_label.setText("Transmitindo tela...")
        )
        self.video_thread.fps_updated.connect(
            lambda fps: self.video_status_label.setText(f"Transmitindo tela... ({fps:.1f} FPS)")
        )
        self.video_thread.error_occurred.connect(
            lambda msg: self.video_status_label.setText(f"Erro na transmissão: {msg}")
        )
        self.video_thread.start()

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        self.video_thread.stop()
        self.video_thread.wait()
        event.accept()


class ClientWindow(QMainWindow):
    def __init__(self, host_ip, username):
        super().__init__()
        self.setWindowTitle("LANShare - Client")
        self.resize(1100, 650)

        self.video_label = QLabel("Conectando ao stream...")
        self.video_label.setScaledContents(True)

        chat_thread = ChatClientThread(username, host_ip)
        self.chat_panel = ChatPanel(username, chat_thread)
        self.chat_panel.setMaximumWidth(320)

        splitter = QSplitter(Qt.Horizontal)
        splitter.addWidget(self.video_label)
        splitter.addWidget(self.chat_panel)
        splitter.setStretchFactor(0, 3)
        splitter.setStretchFactor(1, 1)

        self.setCentralWidget(splitter)

        self.video_thread = VideoReceiveThread(host_ip)
        self.video_thread.frame_received.connect(self._update_frame)
        self.video_thread.connection_lost.connect(
            lambda: self.video_label.setText("Conexão com o stream perdida.")
        )
        self.video_thread.start()

    def _update_frame(self, frame_bytes):
        image = QImage.fromData(frame_bytes, "JPG")
        pixmap = QPixmap.fromImage(image)
        self.video_label.setPixmap(pixmap)

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        self.video_thread.stop()
        self.video_thread.wait()
        event.accept()