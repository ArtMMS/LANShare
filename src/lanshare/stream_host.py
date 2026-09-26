import socket
import time

import dxcam
import mss

from streaming import send_frame
from screen_capture import choose_monitor, draw_cursor, compress_frame

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
        monitor_index, monitor = choose_monitor(sct)

    # dxcam usa índice começando em 0 para o monitor primário;
    # o mss usa 1 para o primeiro monitor real (0 é "todos juntos") — por isso o -1
    camera = dxcam.create(output_idx=monitor_index - 1, output_color="BGR")

    frames_since_report = 0
    last_report_time = time.time()

    try:
        while True:
            frame_start = time.time()

            frame = camera.grab()
            if frame is None:
                # ainda não há frame novo desde a última captura; espera um pouco e tenta de novo
                time.sleep(0.001)
                continue

            draw_cursor(frame, monitor)
            frame_bytes = compress_frame(frame, verbose=False)
            send_frame(conn, frame_bytes)

            frames_since_report += 1
            now = time.time()
            if now - last_report_time >= 1.0:
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