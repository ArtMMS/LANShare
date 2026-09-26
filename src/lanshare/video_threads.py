import socket
import time

import dxcam
from PySide6.QtCore import QThread, Signal

from streaming import send_frame, receive_frame
from screen_capture import compress_frame
from window_selector import get_window_region, get_current_monitor_index

STREAM_PORT = 5556
TARGET_FPS = 30
FRAME_INTERVAL = 1 / TARGET_FPS


class VideoSendServerThread(QThread):
    """Usado pelo Host: espera o Client conectar no vídeo e começa a transmitir."""

    client_connected = Signal()
    fps_updated = Signal(float)
    error_occurred = Signal(str)

    def __init__(self, monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors):
        super().__init__()
        self.monitor_index = monitor_index
        self.monitor = monitor
        self.target_hwnd = target_hwnd
        self.monitor_mapping = monitor_mapping
        self.mss_monitors = mss_monitors
        self._running = True

    def run(self):
        server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server_socket.bind(("0.0.0.0", STREAM_PORT))
        server_socket.listen(1)

        try:
            conn, addr = server_socket.accept()
        except OSError:
            return
        self.client_connected.emit()

        camera = dxcam.create(output_idx=self.monitor_mapping[self.monitor_index], output_color="BGR")

        frames_since_report = 0
        last_report_time = time.time()

        try:
            while self._running:
                frame_start = time.time()

                if self.target_hwnd is not None:
                    current_index = get_current_monitor_index(self.target_hwnd, self.mss_monitors)
                    if (current_index is not None and current_index != self.monitor_index
                            and current_index in self.monitor_mapping):
                        del camera
                        self.monitor_index = current_index
                        self.monitor = self.mss_monitors[current_index]
                        camera = dxcam.create(
                            output_idx=self.monitor_mapping[self.monitor_index], output_color="BGR"
                        )

                    region = get_window_region(self.target_hwnd, self.monitor)
                    if region is None:
                        time.sleep(0.01)
                        continue
                    try:
                        frame = camera.grab(region=region)
                    except Exception:
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
                    fps = frames_since_report / (now - last_report_time)
                    self.fps_updated.emit(fps)
                    frames_since_report = 0
                    last_report_time = now

                elapsed = time.time() - frame_start
                sleep_time = FRAME_INTERVAL - elapsed
                if sleep_time > 0:
                    time.sleep(sleep_time)

        except (ConnectionResetError, OSError) as e:
            self.error_occurred.emit(str(e))
        finally:
            conn.close()
            server_socket.close()

    def stop(self):
        self._running = False


class VideoReceiveThread(QThread):
    """Usado pelo Client: conecta no vídeo do Host e recebe os frames."""

    frame_received = Signal(bytes)
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