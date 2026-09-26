import socket
import time
import mss
from streaming import send_frame
from screen_capture import choose_monitor, capture_and_compress

STREAM_PORT = 5556
TARGET_FPS = 30
FRAME_INTERVAL = 1 / TARGET_FPS


def start_stream_host():
    server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server_socket.bind(("0.0.0.0", STREAM_PORT))
    server_socket.listen(1)

    print(f"[STREAM] Aguardando conexão de vídeo na porta {STREAM_PORT}...")
    conn, addr = server_socket.accept()
    print(f"[STREAM] Cliente de vídeo conectado: {addr}")

    with mss.mss() as sct:
        monitor = choose_monitor(sct)

        frames_since_report = 0
        last_report_time = time.time()

        try:
            while True:
                frame_start = time.time()

                frame_bytes = capture_and_compress(monitor, sct, verbose=False)
                send_frame(conn, frame_bytes)

                frames_since_report += 1
                now = time.time()
                if now - last_report_time >= 1.0:  # reporta a cada 1 segundo real
                    fps_real = frames_since_report / (now - last_report_time)
                    print(f"[STREAM] FPS médio: {fps_real:.1f}")
                    frames_since_report = 0
                    last_report_time = now

                elapsed_frame = time.time() - frame_start
                sleep_time = FRAME_INTERVAL - elapsed_frame
                if sleep_time > 0:
                    time.sleep(sleep_time)

        except (ConnectionResetError, OSError):
            print("[STREAM] Cliente de vídeo desconectou.")
        finally:
            conn.close()
            server_socket.close()


if __name__ == "__main__":
    start_stream_host()