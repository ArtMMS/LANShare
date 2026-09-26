import socket
import time

from PySide6.QtCore import QThread, Signal

CHAT_PORT = 5555
HEARTBEAT_INTERVAL = 5
TIMEOUT = 12


class BaseChatThread(QThread):
    peer_connected = Signal(str)
    message_received = Signal(str, str)  # username, texto
    status_changed = Signal(str)
    disconnected = Signal(str)  # motivo

    def __init__(self, my_username):
        super().__init__()
        self.my_username = my_username
        self.peer_username = "Peer"
        self.conn = None
        self.last_seen = time.time()
        self._running = True

    def _handshake(self, conn, is_host):
        if is_host:
            peer_username = conn.recv(1024).decode("utf-8")
            conn.sendall(self.my_username.encode("utf-8"))
        else:
            conn.sendall(self.my_username.encode("utf-8"))
            peer_username = conn.recv(1024).decode("utf-8")
        return peer_username

    def _receive_loop(self):
        while self._running:
            try:
                data = self.conn.recv(1024)
                if not data:
                    self.disconnected.emit("conexão perdida")
                    break
                text = data.decode("utf-8")
                self.last_seen = time.time()
                if text == "__PING__":
                    continue
                if text == "__DISCONNECT__":
                    self.disconnected.emit(f"{self.peer_username} saiu da rede")
                    break
                self.message_received.emit(self.peer_username, text)
            except (ConnectionResetError, OSError):
                self.disconnected.emit("conexão perdida")
                break

    def send_message(self, text):
        if self.conn:
            try:
                self.conn.sendall(text.encode("utf-8"))
            except OSError:
                pass

    def send_ping(self):
        if self.conn:
            try:
                self.conn.sendall("__PING__".encode("utf-8"))
            except OSError:
                pass

    def send_disconnect_signal(self):
        if self.conn:
            try:
                self.conn.sendall("__DISCONNECT__".encode("utf-8"))
            except OSError:
                pass

    def check_timeout(self):
        if self._running and (time.time() - self.last_seen) > TIMEOUT:
            self._running = False
            self.disconnected.emit("sem resposta (timeout)")

    def stop(self):
        self._running = False
        if self.conn:
            try:
                self.conn.close()
            except OSError:
                pass


class ChatServerThread(BaseChatThread):
    """Usado pelo Host: fica esperando o Client se conectar no chat."""

    def run(self):
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server_socket.bind(("0.0.0.0", CHAT_PORT))
        server_socket.listen(1)
        self.status_changed.emit(f"Aguardando conexão na porta {CHAT_PORT}...")

        try:
            conn, addr = server_socket.accept()
        except OSError:
            return

        self.conn = conn
        self.peer_username = self._handshake(conn, is_host=True)
        self.last_seen = time.time()
        self.peer_connected.emit(self.peer_username)

        self._receive_loop()
        server_socket.close()


class ChatClientThread(BaseChatThread):
    """Usado pelo Client: conecta no chat do Host."""

    def __init__(self, my_username, host_ip):
        super().__init__(my_username)
        self.host_ip = host_ip

    def run(self):
        client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            client_socket.connect((self.host_ip, CHAT_PORT))
        except OSError:
            self.disconnected.emit("não foi possível conectar")
            return

        self.conn = client_socket
        self.peer_username = self._handshake(client_socket, is_host=False)
        self.last_seen = time.time()
        self.peer_connected.emit(self.peer_username)

        self._receive_loop()