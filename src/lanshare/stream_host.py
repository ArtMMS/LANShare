import socket
import time
import mss
import numpy as np
from streaming import send_frame
from screen_capture import choose_monitor, draw_cursor
import cv2

STREAM_PORT = 5556
TARGET_FPS = 30
FRAME_INTERVAL = 1 / TARGET_FPS
JPEG_QUALITY = 70


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
        t_capture_total = t_cursor_total = t_encode_total = t_send_total = 0.0

        try:
            while True:
                frame_start = time.time()

                t0 = time.time()
                screenshot = sct.grab(monitor)
                t1 = time.time()

                frame = np.ascontiguousarray(np.array(screenshot, dtype=np.uint8)[:, :, :3])
                t2 = time.time()

                draw_cursor(frame, monitor)
                t3 = time.time()

                success, encoded = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, JPEG_QUALITY])
                frame_bytes = encoded.tobytes()
                t4 = time.time()

                send_frame(conn, frame_bytes)
                t5 = time.time()

                t_capture_total += (t1 - t0)
                t_cursor_total += (t3 - t2)
                t_encode_total += (t4 - t3)
                t_send_total += (t5 - t4)

                frames_since_report += 1
                now = time.time()
                if now - last_report_time >= 1.0:
                    fps_real = frames_since_report / (now - last_report_time)
                    print(f"[STREAM] FPS médio: {fps_real:.1f} | "
                          f"captura: {t_capture_total*1000/frames_since_report:.1f}ms | "
                          f"cursor: {t_cursor_total*1000/frames_since_report:.1f}ms | "
                          f"encode: {t_encode_total*1000/frames_since_report:.1f}ms | "
                          f"envio: {t_send_total*1000/frames_since_report:.1f}ms")
                    frames_since_report = 0
                    last_report_time = now
                    t_capture_total = t_cursor_total = t_encode_total = t_send_total = 0.0

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