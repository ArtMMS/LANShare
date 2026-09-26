import socket
from streaming import receive_frame

STREAM_PORT = 5556


def start_stream_client(host_ip):
    client_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    client_socket.connect((host_ip, STREAM_PORT))
    print(f"[STREAM] Conectado ao stream de vídeo em {host_ip}:{STREAM_PORT}")

    frame_count = 0

    try:
        while True:
            frame_bytes = receive_frame(client_socket)
            if frame_bytes is None:
                print("[STREAM] Conexão de vídeo encerrada.")
                break

            frame_count += 1
            # por enquanto, só salva o frame mais recente para confirmar que está chegando
            with open("received_frame.jpg", "wb") as f:
                f.write(frame_bytes)

            if frame_count % 30 == 0:
                print(f"[STREAM] {frame_count} frames recebidos até agora.")

    except (ConnectionResetError, OSError):
        print("[STREAM] Conexão de vídeo perdida.")
    finally:
        client_socket.close()


if __name__ == "__main__":
    host_ip = input("Digite o IP do Host para o stream: ").strip()
    start_stream_client(host_ip)