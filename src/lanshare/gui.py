import sys
import mss
import ipaddress
import socket


from PySide6.QtCore import Qt, QTimer
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import (
    QMainWindow, QWidget, QLabel, QTextEdit, QLineEdit,
    QPushButton, QVBoxLayout, QHBoxLayout, QFrame, QSlider,
    QDialog, QListWidget, QListWidgetItem, QRadioButton,
    QComboBox, QDialogButtonBox, QApplication
)

from PySide6.QtWidgets import QMessageBox  # adicione QMessageBox à linha de import já existente
from chat_threads import ChatServerThread, ChatClientThread, CHAT_PORT

from chat_threads import ChatServerThread, ChatClientThread
from video_threads import VideoSendServerThread, VideoReceiveThread
from screen_capture import (
    list_monitors, capture_monitor_preview, RESOLUTION_PRESETS, FPS_PRESETS, BITRATE_PRESETS
)
from window_selector import windows_in_monitor
from monitor_mapping import auto_map_monitors


DARK_STYLESHEET = """
QMainWindow, QWidget, QDialog { background-color: #1b1f2b; color: #e6e8ef; font-family: 'Segoe UI', sans-serif; }
QFrame#sidebar { background-color: #20242f; border-right: 1px solid #2c313f; }
QLabel#appTitle { font-size: 16px; font-weight: 600; color: #ffffff; }
QLabel#sectionLabel { font-size: 11px; font-weight: 600; color: #8b93a7; margin-top: 10px; }
QLabel#statusText { font-size: 13px; color: #ffffff; }
QLabel#dimText { color: #8b93a7; font-size: 12px; }
QFrame#statusDot { background-color: #35d07f; border-radius: 6px; min-width: 12px; max-width: 12px; min-height: 12px; max-height: 12px; }
QFrame#statusDotOff { background-color: #6b7280; border-radius: 6px; min-width: 12px; max-width: 12px; min-height: 12px; max-height: 12px; }
QPushButton { background-color: #2c313f; color: #e6e8ef; border-radius: 6px; padding: 8px 14px; }
QPushButton:hover { background-color: #384056; }
QPushButton:disabled { color: #565d70; }
QPushButton#startButton { background-color: #2f7a4f; }
QPushButton#startButton:hover { background-color: #368a59; }
QPushButton#stopButton { background-color: #7a3030; }
QPushButton#stopButton:hover { background-color: #8a3838; }
QPushButton#primaryButton { background-color: #2f5fa8; font-size: 14px; padding: 12px 20px; }
QPushButton#primaryButton:hover { background-color: #3a6fbf; }
QTextEdit, QLineEdit, QListWidget, QComboBox { background-color: #262b38; border: 1px solid #343b4c; border-radius: 6px; color: #e6e8ef; }
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


def numpy_bgr_to_pixmap(frame, max_width=280):
    import cv2
    height, width = frame.shape[:2]
    scale = max_width / width
    small = cv2.resize(frame, (max_width, int(height * scale)))
    rgb = cv2.cvtColor(small, cv2.COLOR_BGR2RGB)
    qimage = QImage(rgb.data, rgb.shape[1], rgb.shape[0], rgb.strides[0], QImage.Format_RGB888)
    return QPixmap.fromImage(qimage)


# ---------------------------------------------------------------------------
# Dialogs de configuração da transmissão (Host)
# ---------------------------------------------------------------------------

class MonitorChoiceDialog(QDialog):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Escolha o monitor")
        self.resize(500, 400)

        self.monitors = list_monitors()
        self.selected_index = None

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Qual monitor você deseja compartilhar?"))

        self.list_widget = QListWidget()
        layout.addWidget(self.list_widget)

        pixmap = None
        for index, monitor in self.monitors:
            frame = capture_monitor_preview(monitor)
            pixmap = numpy_bgr_to_pixmap(frame, max_width=200)
            item = QListWidgetItem(f"Monitor {index} — {monitor['width']}x{monitor['height']}")
            item.setData(Qt.UserRole, index)
            from PySide6.QtGui import QIcon
            item.setIcon(QIcon(pixmap))
            self.list_widget.addItem(item)

        if pixmap is not None:
            self.list_widget.setIconSize(pixmap.size())

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self._on_accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

        if self.list_widget.count() > 0:
            self.list_widget.setCurrentRow(0)

    def _on_accept(self):
        item = self.list_widget.currentItem()
        if item is None:
            return
        self.selected_index = item.data(Qt.UserRole)
        self.accept()

    def get_selected_monitor(self):
        for index, monitor in self.monitors:
            if index == self.selected_index:
                return index, monitor
        return None, None


class CaptureTargetDialog(QDialog):
    def __init__(self, monitor):
        super().__init__()
        self.setWindowTitle("O que deseja compartilhar?")
        self.resize(420, 380)
        self.monitor = monitor
        self.selected_hwnd = None

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Escolha o que transmitir:"))

        self.full_monitor_radio = QRadioButton("Monitor inteiro")
        self.full_monitor_radio.setChecked(True)
        layout.addWidget(self.full_monitor_radio)

        layout.addWidget(QLabel("Ou uma janela específica:"))
        self.window_list = QListWidget()
        self.windows = windows_in_monitor(monitor)
        for hwnd, title in self.windows:
            item = QListWidgetItem(title)
            item.setData(Qt.UserRole, hwnd)
            self.window_list.addItem(item)
        layout.addWidget(self.window_list)

        self.window_list.itemClicked.connect(lambda _: self.full_monitor_radio.setChecked(False))
        self.full_monitor_radio.toggled.connect(
            lambda checked: self.window_list.clearSelection() if checked else None
        )

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.accepted.connect(self._on_accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _on_accept(self):
        if self.full_monitor_radio.isChecked():
            self.selected_hwnd = None
        else:
            item = self.window_list.currentItem()
            self.selected_hwnd = item.data(Qt.UserRole) if item else None
        self.accept()


class StreamSettingsDialog(QDialog):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Configurações da transmissão")
        self.resize(380, 320)

        layout = QVBoxLayout(self)

        layout.addWidget(QLabel("Resolução da transmissão:"))
        self.resolution_combo = QComboBox()
        self.resolution_combo.addItems(list(RESOLUTION_PRESETS.keys()))
        self.resolution_combo.setCurrentIndex(1)  # 1080p por padrão
        layout.addWidget(self.resolution_combo)

        layout.addWidget(QLabel("Taxa de quadros (FPS):"))
        self.fps_combo = QComboBox()
        self.fps_combo.addItems(list(FPS_PRESETS.keys()))
        self.fps_combo.setCurrentIndex(1)  # 30 FPS por padrão
        layout.addWidget(self.fps_combo)

        layout.addWidget(QLabel("Meta de bitrate:"))
        self.bitrate_combo = QComboBox()
        self.bitrate_combo.addItems(list(BITRATE_PRESETS.keys()))
        self.bitrate_combo.setCurrentIndex(1)
        layout.addWidget(self.bitrate_combo)

        note = QLabel(
            "Nota: 60 FPS e resoluções acima de 1080p exigem mais do hardware "
            "e podem não ser totalmente atingidos sem aceleração por GPU."
        )
        note.setObjectName("dimText")
        note.setWordWrap(True)
        layout.addWidget(note)

        layout.addStretch()

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.button(QDialogButtonBox.Ok).setText("Iniciar Transmissão")
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def get_settings(self):
        target_height = RESOLUTION_PRESETS[self.resolution_combo.currentText()]
        target_fps = FPS_PRESETS[self.fps_combo.currentText()]
        target_bitrate = BITRATE_PRESETS[self.bitrate_combo.currentText()]
        return target_height, target_fps, target_bitrate


class JoinStreamDialog(QDialog):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Entrar em uma transmissão")
        self.resize(340, 180)

        layout = QVBoxLayout(self)

        layout.addWidget(QLabel("IP do Host:"))
        self.ip_input = QLineEdit()
        self.ip_input.setPlaceholderText("ex: 192.168.1.10")
        layout.addWidget(self.ip_input)

        layout.addWidget(QLabel("Seu nome de usuário:"))
        self.username_input = QLineEdit()
        layout.addWidget(self.username_input)

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.button(QDialogButtonBox.Ok).setText("Entrar")
        buttons.accepted.connect(self._on_accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def _on_accept(self):
        ip_text = self.ip_input.text().strip()

        if not self._is_valid_ip(ip_text):
            QMessageBox.warning(
                self, "IP inválido",
                "Digite um endereço IP válido (ex: 192.168.1.10)."
            )
            return

        if not self._host_reachable(ip_text):
            QMessageBox.warning(
                self, "Host não encontrado",
                f"Não foi possível encontrar um LANShare Host em {ip_text}.\n"
                "Verifique o IP e se o Host está com o aplicativo aberto."
            )
            return

        self.accept()

    def _is_valid_ip(self, ip_text):
        try:
            ipaddress.ip_address(ip_text)
            return True
        except ValueError:
            return False

    def _host_reachable(self, ip_text, timeout=2.0):
        """Testa uma conexão real e rápida na porta do chat, que fica ativa assim
        que o Host abre o app — mesmo antes de ele começar a transmitir vídeo."""
        try:
            with socket.create_connection((ip_text, CHAT_PORT), timeout=timeout):
                return True
        except OSError:
            return False

    def get_values(self):
        return self.ip_input.text().strip(), (self.username_input.text().strip() or "Client")


class HostSetupDialog(QDialog):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Nome de usuário")
        self.resize(320, 140)

        layout = QVBoxLayout(self)
        layout.addWidget(QLabel("Seu nome de usuário:"))
        self.username_input = QLineEdit()
        layout.addWidget(self.username_input)

        buttons = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        buttons.button(QDialogButtonBox.Ok).setText("Continuar")
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def get_username(self):
        return self.username_input.text().strip() or "Host"


# ---------------------------------------------------------------------------
# Launcher — primeira tela ao abrir o app
# ---------------------------------------------------------------------------

class LauncherWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("LANShare")
        self.resize(480, 320)
        self.setStyleSheet(DARK_STYLESHEET)

        self.host_window = None
        self.client_window = None

        container = QWidget()
        layout = QVBoxLayout(container)
        layout.addStretch()

        title = QLabel("LANShare")
        title.setObjectName("appTitle")
        title.setStyleSheet("font-size: 26px;")
        title.setAlignment(Qt.AlignCenter)
        layout.addWidget(title)

        subtitle = QLabel("Compartilhamento de tela em rede local")
        subtitle.setObjectName("dimText")
        subtitle.setAlignment(Qt.AlignCenter)
        layout.addWidget(subtitle)
        layout.addSpacing(24)

        self.transmit_button = QPushButton("📡 Transmitir minha tela")
        self.transmit_button.setObjectName("primaryButton")
        self.transmit_button.clicked.connect(self._on_transmit_clicked)
        layout.addWidget(self.transmit_button)

        self.join_button = QPushButton("👁 Entrar em uma transmissão")
        self.join_button.setObjectName("primaryButton")
        self.join_button.clicked.connect(self._on_join_clicked)
        layout.addWidget(self.join_button)

        layout.addStretch()
        self.setCentralWidget(container)

    def _on_transmit_clicked(self):
        """Só pede o nome de usuário aqui. A escolha de monitor/janela/qualidade
        acontece dentro da HostWindow, ao clicar em 'Start Streaming' — assim
        dá para refazer essas escolhas toda vez que a transmissão for reiniciada."""
        username_dialog = HostSetupDialog()
        if username_dialog.exec() != QDialog.Accepted:
            return
        username = username_dialog.get_username()

        self.host_window = HostWindow(username)
        self.host_window.show()
        self.close()

    def _on_join_clicked(self):
        dialog = JoinStreamDialog()
        if dialog.exec() != QDialog.Accepted:
            return
        host_ip, username = dialog.get_values()
        if not host_ip:
            return

        self.client_window = ClientWindow(host_ip, username)
        self.client_window.show()
        self.close()


# ---------------------------------------------------------------------------
# Vídeo com badge "LANShare Live" e placeholder de "aguardando"
# ---------------------------------------------------------------------------

class VideoContainer(QWidget):
    def __init__(self, placeholder_text="Aguardando stream..."):
        super().__init__()
        self.placeholder_text = placeholder_text

        self.video_label = QLabel(placeholder_text)
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

    def show_placeholder(self, text=None):
        """Limpa o vídeo e volta a exibir o texto de espera (ex: ao parar a transmissão)."""
        self.video_label.setPixmap(QPixmap())
        self.video_label.setText(text or self.placeholder_text)
        self.set_live(False)

    def resizeEvent(self, event):
        self.video_label.setGeometry(0, 0, self.width(), self.height())
        margin = 14
        self.badge.move(self.width() - self.badge.width() - margin, margin)
        super().resizeEvent(event)


# ---------------------------------------------------------------------------
# Chat
# ---------------------------------------------------------------------------

class ChatPanel(QWidget):
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


# ---------------------------------------------------------------------------
# Sidebar
# ---------------------------------------------------------------------------

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

        self.layout.addWidget(self._section_label("STATUS"))
        status_row = QHBoxLayout()
        self.status_dot = status_dot(False)
        self.status_text = QLabel(role_label)
        self.status_text.setObjectName("statusText")
        status_row.addWidget(self.status_dot)
        status_row.addWidget(self.status_text)
        status_row.addStretch()
        self.layout.addLayout(status_row)

        self.layout.addWidget(self._section_label("CONEXÃO"))
        self.connection_label = QLabel("—")
        self.connection_label.setObjectName("dimText")
        self.layout.addWidget(self.connection_label)

        self.layout.addWidget(self._section_label("USUÁRIOS"))
        self.users_layout = QVBoxLayout()
        self.layout.addLayout(self.users_layout)
        self.add_user(f"{username} (Você)")

        self.layout.addWidget(self._section_label("CONTROLES DE ÁUDIO (V0.5)"))
        audio_row = QHBoxLayout()
        audio_slider = QSlider(Qt.Horizontal)
        audio_slider.setEnabled(False)
        mute_button = QPushButton("Mute")
        mute_button.setEnabled(False)
        audio_row.addWidget(audio_slider)
        audio_row.addWidget(mute_button)
        self.layout.addLayout(audio_row)

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

        self.chat_toggle = QPushButton("💬 Mostrar chat")
        self.layout.addWidget(self.chat_toggle)
        self.chat_container = QWidget()
        self.chat_container.setVisible(False)
        self.chat_container_layout = QVBoxLayout(self.chat_container)
        self.chat_container_layout.setContentsMargins(0, 8, 0, 0)
        self.layout.addWidget(self.chat_container)
        self.chat_toggle.clicked.connect(self._toggle_chat)

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

    def reset_stream_info(self):
        self.fps_label.setText("FPS: —")
        self.latency_label.setText("Latência: —")
        self.bitrate_label.setText("Bitrate: —")

    def set_status(self, active, text):
        self.status_dot.setObjectName("statusDot" if active else "statusDotOff")
        self.status_dot.setStyleSheet(DARK_STYLESHEET)
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


# ---------------------------------------------------------------------------
# HostWindow — abre parado ("Aguardando stream"); Start reconfigura tudo;
# Stop encerra a captura por completo e volta ao estado inicial
# ---------------------------------------------------------------------------

class HostWindow(QMainWindow):
    def __init__(self, username):
        super().__init__()
        self.setWindowTitle("LANShare - Host")
        self.resize(1200, 700)
        self.setStyleSheet(DARK_STYLESHEET)
        self.username = username

        self.sidebar = Sidebar(username, "Host (Aguardando)")
        self.sidebar.set_connection("Nenhuma transmissão ativa")

        self.video_container = VideoContainer(placeholder_text="Aguardando stream...")

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

        self.video_thread = None  # só existe enquanto a transmissão está ativa

    def _on_start(self):
        """Reabre a configuração completa (monitor, janela, resolução, bitrate)
        toda vez que a transmissão é (re)iniciada."""
        monitor_dialog = MonitorChoiceDialog()
        if monitor_dialog.exec() != QDialog.Accepted:
            return
        monitor_index, monitor = monitor_dialog.get_selected_monitor()
        if monitor is None:
            return

        target_dialog = CaptureTargetDialog(monitor)
        if target_dialog.exec() != QDialog.Accepted:
            return
        target_hwnd = target_dialog.selected_hwnd

        settings_dialog = StreamSettingsDialog()
        if settings_dialog.exec() != QDialog.Accepted:
            return
        target_height, target_fps, target_bitrate = settings_dialog.get_settings()

        with mss.mss() as sct:
            mss_monitors = sct.monitors
        monitor_mapping = auto_map_monitors(mss_monitors)

        self.video_thread = VideoSendServerThread(
            monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors,
            target_height=target_height, target_fps=target_fps, target_bitrate_kbps=target_bitrate
        )
        self.video_thread.client_connected.connect(self._on_client_connected)
        self.video_thread.client_disconnected.connect(self._on_client_disconnected)
        self.video_thread.fps_updated.connect(lambda fps: self.sidebar.set_stream_info(fps=fps))
        self.video_thread.stats_updated.connect(
            lambda kbps, quality: self.sidebar.set_stream_info(bitrate_kbps=kbps)
        )
        self.video_thread.frame_captured.connect(self._on_frame_captured)
        self.video_thread.error_occurred.connect(self._on_stream_error)
        self.video_thread.start()

        self.start_button.setEnabled(False)
        self.stop_button.setEnabled(True)
        self.video_container.set_live(True)
        self.sidebar.set_status(True, "Host (Broadcasting)")
        self.sidebar.set_connection("Transmitindo — aguardando visualizadores")

    def _on_stop(self):
        """Encerra a transmissão por completo. Volta ao estado inicial:
        placeholder 'Aguardando stream' e Start Streaming disponível de novo."""
        if self.video_thread:
            self.video_thread.stop()
            self.video_thread.wait()
            self.video_thread = None

        self.video_container.show_placeholder("Aguardando stream...")
        self.sidebar.set_status(False, "Host (Aguardando)")
        self.sidebar.set_connection("Nenhuma transmissão ativa")
        self.sidebar.reset_stream_info()

        self.start_button.setEnabled(True)
        self.stop_button.setEnabled(False)

    def _on_frame_captured(self, frame_bytes):
        """Alimenta a prévia do próprio Host com os mesmos frames que estão sendo enviados."""
        self.video_container.update_frame(frame_bytes)

    def _on_client_connected(self):
        self.sidebar.set_connection("Client(s) assistindo")

    def _on_client_disconnected(self):
        self.sidebar.set_connection("Transmitindo — aguardando visualizadores")

    def _on_stream_error(self, msg):
        self.sidebar.set_status(False, f"Erro: {msg}")

    def _on_peer_connected(self, peer_username):
        self.sidebar.add_user(f"{peer_username} (Client)")

    def _on_peer_disconnected(self, reason):
        pass  # o vídeo continua rodando mesmo sem o chat conectado

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        if self.video_thread:
            self.video_thread.stop()
            self.video_thread.wait()
        event.accept()


# ---------------------------------------------------------------------------
# ClientWindow — pode sair e entrar de novo livremente
# ---------------------------------------------------------------------------

class ClientWindow(QMainWindow):
    def __init__(self, host_ip, username):
        super().__init__()
        self.setWindowTitle("LANShare - Client")
        self.resize(1200, 700)
        self.setStyleSheet(DARK_STYLESHEET)
        self.host_ip = host_ip
        self.username = username

        self.sidebar = Sidebar(username, "Client (Conectando...)")
        self.sidebar.set_connection(host_ip)

        self.video_container = VideoContainer(placeholder_text="Conectando ao stream...")

        self.stream_toggle_button = QPushButton("🚪 Sair da transmissão")
        self.stream_toggle_button.clicked.connect(self._on_toggle_stream)
        self.sidebar.controls_layout.addWidget(self.stream_toggle_button)
        self._watching = True

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
        self.video_thread = None

        self._start_video_thread()

    def _start_video_thread(self):
        self.video_thread = VideoReceiveThread(self.host_ip)
        self.video_thread.frame_received.connect(self._on_frame_received)
        self.video_thread.connection_lost.connect(self._on_connection_lost)
        self.video_thread.reconnected.connect(self._on_reconnected)
        self.video_thread.start()

    def _on_peer_connected(self, peer_username):
        self.sidebar.set_status(True, "Client (Viewing)")
        self.sidebar.add_user(f"{peer_username} (Host)")

    def _on_peer_disconnected(self, reason):
        self.sidebar.set_status(False, f"Client ({reason})")

    def _on_frame_received(self, frame_bytes, latency):
        import time
        self.video_container.update_frame(frame_bytes)
        self.video_container.set_live(True)

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
        """A transmissão caiu (ex: Host clicou em Stop Streaming). Não é preciso
        nenhuma ação do usuário — a thread já está tentando reconectar sozinha
        em segundo plano."""
        self.video_container.show_placeholder("Transmissão interrompida. Reconectando automaticamente...")

    def _on_reconnected(self):
        """A thread conseguiu reconectar sozinha após uma queda."""
        pass  # a própria chegada de novos frames já atualiza o vídeo e o status

    def _on_toggle_stream(self):
        """Ação explícita do usuário: sair da transmissão de propósito, ou
        voltar a assistir depois de ter saído. Diferente da reconexão automática
        acima, que cobre o Host parando/reiniciando a transmissão sozinho."""
        if self._watching:
            if self.video_thread:
                self.video_thread.stop()
                self.video_thread.wait()
            self.video_container.show_placeholder("Você saiu da transmissão.")
            self.stream_toggle_button.setText("🔄 Entrar novamente")
            self._watching = False
        else:
            self.video_container.show_placeholder("Conectando ao stream...")
            self._start_video_thread()
            self.stream_toggle_button.setText("🚪 Sair da transmissão")
            self._watching = True

    def closeEvent(self, event):
        self.chat_panel.shutdown()
        if self.video_thread:
            self.video_thread.stop()
            self.video_thread.wait()
        event.accept()