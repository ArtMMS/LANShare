import sys
from PySide6.QtCore import QThread, Signal
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import QApplication, QLabel, QMainWindow, QVBoxLayout, QWidget

import socket
from streaming import receive_frame

STREAM_PORT = 5556


class VideoReceiverThread(QThread):
    frame_received = Signal(bytes)  # sinal que avisa a GUI: "chegou um frame novo"
    connection_lost = Signal()

    def __init__(self, host_ip):
        super().__init__()
        self.host_ip = host_ip
        self._running = True

    def run(self):
        client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            client_socket.connect((self.host_ip, STREAM_PORT))
        except OSError:
            self.connection_lost.emit()
            return

        while self._running:
            frame_bytes = receive_frame(client_socket)
            if frame_bytes is None:
                self.connection_lost.emit()
                break
            self.frame_received.emit(frame_bytes)

        client_socket.close()

    def stop(self):
        self._running = False


class StreamWindow(QMainWindow):
    def __init__(self, host_ip):
        super().__init__()
        self.setWindowTitle("LANShare - Tela do Host")
        self.resize(1000, 600)

        self.video_label = QLabel("Conectando ao stream...")
        self.video_label.setScaledContents(True)  # a imagem se ajusta ao tamanho da janela

        container = QWidget()
        layout = QVBoxLayout(container)
        layout.addWidget(self.video_label)
        self.setCentralWidget(container)

        self.receiver = VideoReceiverThread(host_ip)
        self.receiver.frame_received.connect(self.update_frame)
        self.receiver.connection_lost.connect(self.handle_disconnect)
        self.receiver.start()

    def update_frame(self, frame_bytes):
        image = QImage.fromData(frame_bytes, "JPG")
        pixmap = QPixmap.fromImage(image)
        self.video_label.setPixmap(pixmap)

    def handle_disconnect(self):
        self.video_label.setText("Conexão com o stream perdida.")

    def closeEvent(self, event):
        self.receiver.stop()
        self.receiver.wait()
        event.accept()


def main():
    host_ip = input("Digite o IP do Host para o stream: ").strip()

    app = QApplication(sys.argv)
    window = StreamWindow(host_ip)
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()