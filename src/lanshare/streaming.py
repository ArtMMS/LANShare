import struct


def send_frame(sock, frame_bytes):
    """Manda um frame prefixado com seu tamanho (4 bytes), para o receptor saber onde ele termina."""
    length = struct.pack(">I", len(frame_bytes))  # 4 bytes, big-endian, número inteiro sem sinal
    sock.sendall(length + frame_bytes)


def receive_frame(sock):
    """Lê um frame completo: primeiro os 4 bytes de tamanho, depois exatamente essa quantidade de dados."""
    length_bytes = recv_exact(sock, 4)
    if length_bytes is None:
        return None
    length = struct.unpack(">I", length_bytes)[0]
    return recv_exact(sock, length)


def recv_exact(sock, num_bytes):
    """Garante ler exatamente num_bytes, já que recv() pode devolver menos do que foi pedido."""
    data = b""
    while len(data) < num_bytes:
        chunk = sock.recv(num_bytes - len(data))
        if not chunk:
            return None  # conexão caiu no meio da leitura
        data += chunk
    return data