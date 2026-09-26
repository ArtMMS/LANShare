import socket
import time

import dxcam
import mss

from streaming import send_frame
from screen_capture import choose_monitor, compress_frame
from window_selector import choose_window, get_window_region

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

    target_hwnd = choose_window(monitor)  # None = monitor inteiro; ou o hwnd de uma janela específica

    camera = dxcam.create(output_idx=monitor_index - 1, output_color="BGR")

    frames_since_report = 0
    last_report_time = time.time()

    try:
        while True:
            frame_start = time.time()

            if target_hwnd is not None:
                region = get_window_region(target_hwnd, monitor)
                if region is None:
                    # janela minimizada, fechada ou fora dos limites do monitor no momento;
                    # pula esse frame em vez de derrubar a conexão
                    time.sleep(0.01)
                    continue
                try:
                    frame = camera.grab(region=region)
                except Exception as e:
                    print(f"[STREAM] Erro ao capturar região da janela: {e}")
                    time.sleep(0.01)
                    continue
            else:
                frame = camera.grab()

            if frame is None:
                time.sleep(0.001)
                continue

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