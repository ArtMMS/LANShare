import socket
import time

from PySide6.QtCore import QThread, Signal

CHAT_PORT = 5555
TIMEOUT = 12


class BaseChatThread(QThread):
    peer_connected = Signal(str)
    message_received = Signal(str, str)
    status_changed = Signal(str)
    disconnected = Signal(str)

    def __init__(self, my_username):
        super().__init__()
        self.my_username = my_username
        self.peer_username = "Peer"
        self.conn = None
        self.last_seen = time.time()
        self.peer_active = False
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
        while self._running and self.peer_active:
            try:
                data = self.conn.recv(1024)
                if not data:
                    self.peer_active = False
                    self.disconnected.emit("conexão perdida")
                    break
                text = data.decode("utf-8")
                self.last_seen = time.time()
                if text == "__PING__":
                    continue
                if text == "__DISCONNECT__":
                    self.peer_active = False
                    self.disconnected.emit(f"{self.peer_username} saiu da rede")
                    break
                self.message_received.emit(self.peer_username, text)
            except (ConnectionResetError, OSError):
                self.peer_active = False
                self.disconnected.emit("conexão perdida")
                break

    def send_message(self, text):
        if self.conn and self.peer_active:
            try:
                self.conn.sendall(text.encode("utf-8"))
            except OSError:
                pass

    def send_ping(self):
        if self.conn and self.peer_active:
            try:
                self.conn.sendall("__PING__".encode("utf-8"))
            except OSError:
                pass

    def send_disconnect_signal(self):
        if self.conn and self.peer_active:
            try:
                self.conn.sendall("__DISCONNECT__".encode("utf-8"))
            except OSError:
                pass

    def check_timeout(self):
        if self.peer_active and (time.time() - self.last_seen) > TIMEOUT:
            self.peer_active = False
            self.disconnected.emit("sem resposta (timeout)")

    def stop(self):
        self._running = False
        self.peer_active = False
        if self.conn:
            try:
                self.conn.close()
            except OSError:
                pass


class ChatServerThread(BaseChatThread):
    """Usado pelo Host: aceita conexões repetidamente, permitindo o Client
    sair e entrar de novo sem derrubar o Host."""

    def run(self):
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server_socket.bind(("0.0.0.0", CHAT_PORT))
        server_socket.listen(1)
        server_socket.settimeout(0.5)
        self.status_changed.emit(f"Aguardando conexão na porta {CHAT_PORT}...")

        while self._running:
            try:
                conn, addr = server_socket.accept()
            except socket.timeout:
                continue
            except OSError:
                break

            self.conn = conn
            try:
                self.peer_username = self._handshake(conn, is_host=True)
            except OSError:
                continue

            self.last_seen = time.time()
            self.peer_active = True
            self.peer_connected.emit(self.peer_username)

            self._receive_loop()
            self.conn = None
            if self._running:
                self.status_changed.emit(f"Aguardando conexão na porta {CHAT_PORT}...")

        server_socket.close()


class ChatClientThread(BaseChatThread):
    """Usado pelo Client: uma nova instância é criada a cada tentativa de entrar."""

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
        self.peer_active = True
        self.peer_connected.emit(self.peer_username)

        self._receive_loop()