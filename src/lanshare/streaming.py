import struct
import time


def send_frame(sock, frame_bytes, timestamp=None):
    """Manda um frame prefixado com seu tamanho (4 bytes) e o instante da captura (8 bytes),
    para o receptor saber onde ele termina e calcular a latência."""
    if timestamp is None:
        timestamp = time.time()
    header = struct.pack(">Id", len(frame_bytes), timestamp)
    sock.sendall(header + frame_bytes)


def receive_frame(sock):
    """Lê um frame completo: tamanho + timestamp, depois os dados. Retorna (frame_bytes, timestamp)
    ou (None, None) se a conexão caiu."""
    header = recv_exact(sock, 12)  # 4 bytes (tamanho) + 8 bytes (timestamp double)
    if header is None:
        return None, None
    length, timestamp = struct.unpack(">Id", header)
    frame_bytes = recv_exact(sock, length)
    if frame_bytes is None:
        return None, None
    return frame_bytes, timestamp


def recv_exact(sock, num_bytes):
    data = b""
    while len(data) < num_bytes:
        chunk = sock.recv(num_bytes - len(data))
        if not chunk:
            return None
        data += chunk
    return data