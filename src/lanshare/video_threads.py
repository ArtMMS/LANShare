import socket
import time

import dxcam
from PySide6.QtCore import QThread, Signal

from streaming import send_frame, receive_frame
from screen_capture import compress_frame, resize_frame
from bitrate_controller import BitrateController
from window_selector import get_window_region, get_current_monitor_index

STREAM_PORT = 5556


class VideoSendServerThread(QThread):
    """Usado pelo Host: transmite continuamente assim que iniciado, mesmo sem
    nenhum Client conectado. FPS e resolução-alvo agora são configuráveis
    por instância, escolhidos nos dialogs de configuração."""

    client_connected = Signal()
    client_disconnected = Signal()
    fps_updated = Signal(float)
    stats_updated = Signal(float, int)
    streaming_state_changed = Signal(bool)
    frame_captured = Signal(bytes)
    error_occurred = Signal(str)

    def __init__(self, monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors,
                 target_height=1080, target_fps=30, target_bitrate_kbps=4000):
        super().__init__()
        self.monitor_index = monitor_index
        self.monitor = monitor
        self.target_hwnd = target_hwnd
        self.monitor_mapping = monitor_mapping
        self.mss_monitors = mss_monitors
        self.target_height = target_height
        self.frame_interval = 1 / target_fps
        self.bitrate_controller = BitrateController(target_bitrate_kbps)
        self.streaming_enabled = True
        self._running = True
        self._clients = []
        self._server_socket = None

    def run(self):
        self._server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server_socket.bind(("0.0.0.0", STREAM_PORT))
        self._server_socket.listen(5)
        self._server_socket.settimeout(0.01)

        try:
            camera = dxcam.create(output_idx=self.monitor_mapping[self.monitor_index], output_color="BGR")
        except Exception as e:
            self.error_occurred.emit(f"Falha ao iniciar captura: {e}")
            return

        frames_since_report = 0
        last_report_time = time.time()

        try:
            while self._running:
                try:
                    conn, addr = self._server_socket.accept()
                    conn.settimeout(None)
                    self._clients.append(conn)
                    self.client_connected.emit()
                except socket.timeout:
                    pass
                except OSError:
                    pass

                if not self.streaming_enabled:
                    time.sleep(0.05)
                    continue

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

                frame = resize_frame(frame, self.target_height)
                current_quality = self.bitrate_controller.quality
                frame_bytes = compress_frame(frame, quality=current_quality, verbose=False)

                self.frame_captured.emit(frame_bytes)

                if self._clients:
                    still_connected = []
                    for client_conn in self._clients:
                        try:
                            send_frame(client_conn, frame_bytes, timestamp=frame_start)
                            still_connected.append(client_conn)
                        except OSError:
                            try:
                                client_conn.close()
                            except OSError:
                                pass
                            self.client_disconnected.emit()
                    self._clients = still_connected

                new_quality = self.bitrate_controller.register_frame(len(frame_bytes))

                frames_since_report += 1
                now = time.time()
                if now - last_report_time >= 1.0:
                    fps = frames_since_report / (now - last_report_time)
                    self.fps_updated.emit(fps)
                    self.stats_updated.emit(self.bitrate_controller.current_bitrate_kbps(), new_quality)
                    frames_since_report = 0
                    last_report_time = now

                elapsed = time.time() - frame_start
                sleep_time = self.frame_interval - elapsed
                if sleep_time > 0:
                    time.sleep(sleep_time)

        except Exception as e:
            self.error_occurred.emit(str(e))
        finally:
            for c in self._clients:
                try:
                    c.close()
                except OSError:
                    pass
            self._server_socket.close()

    def stop(self):
        self._running = False


class VideoReceiveThread(QThread):
    """Usado pelo Client: conecta no vídeo do Host. Uma nova instância é criada
    a cada tentativa de entrar, permitindo sair e entrar de novo livremente."""

    frame_received = Signal(bytes, float)
    connection_lost = Signal()

    def __init__(self, host_ip):
        super().__init__()
        self.host_ip = host_ip
        self._running = True
        self.socket = None

    def run(self):
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            self.socket.connect((self.host_ip, STREAM_PORT))
        except OSError:
            self.connection_lost.emit()
            return

        while self._running:
            frame_bytes, timestamp = receive_frame(self.socket)
            if frame_bytes is None:
                if self._running:
                    self.connection_lost.emit()
                break
            latency = max(0.0, time.time() - timestamp)
            self.frame_received.emit(frame_bytes, latency)

        try:
            self.socket.close()
        except OSError:
            pass

    def stop(self):
        self._running = False
        if self.socket:
            try:
                self.socket.close()
            except OSError:
                pass