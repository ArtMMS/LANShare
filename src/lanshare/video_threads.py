import socket
import time

import dxcam
from PySide6.QtCore import QThread, Signal

from streaming import send_frame, receive_frame
from screen_capture import resize_frame
from video_codec import create_encoder, create_decoder, encode_frame, decode_packet
from window_selector import get_window_region, get_current_monitor_index

STREAM_PORT = 5556
RECONNECT_RETRY_INTERVAL = 2.0


class VideoSendServerThread(QThread):
    """Usado pelo Host: transmite vídeo H.264 continuamente, acelerado por GPU
    quando disponível (Nvidia/AMD/Intel), com fallback automático para
    software. Usa o modo de captura contínua do dxcam (video_mode=True)."""

    client_connected = Signal()
    client_disconnected = Signal()
    fps_updated = Signal(float)
    stats_updated = Signal(float)
    encoder_selected = Signal(str)
    frame_captured = Signal(object)
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
        self.target_fps = target_fps
        self.target_bitrate_kbps = target_bitrate_kbps
        self._running = True
        self._clients = []
        self._server_socket = None

    def _current_region(self):
        if self.target_hwnd is None:
            return None
        return get_window_region(self.target_hwnd, self.monitor)

    def _start_capture(self, camera, region):
        camera.start(region=region, target_fps=self.target_fps, video_mode=True)

    def run(self):
        self._server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._server_socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._server_socket.bind(("0.0.0.0", STREAM_PORT))
        self._server_socket.listen(5)
        self._server_socket.setblocking(False)

        try:
            camera = dxcam.create(output_idx=self.monitor_mapping[self.monitor_index], output_color="BGR")
        except Exception as e:
            self.error_occurred.emit(f"Falha ao iniciar captura: {e}")
            return

        current_region = self._current_region()
        try:
            self._start_capture(camera, current_region)
        except Exception as e:
            self.error_occurred.emit(f"Falha ao iniciar captura contínua: {e}")
            return

        first_frame = None
        for _ in range(300):
            raw = camera.get_latest_frame()
            if raw is not None:
                first_frame = resize_frame(raw, self.target_height)
                break
            time.sleep(0.005)

        if first_frame is None:
            self.error_occurred.emit("Não foi possível capturar o primeiro frame.")
            camera.stop()
            return

        height, width = first_frame.shape[:2]
        try:
            encoder_ctx, encoder_name, encoder_label = create_encoder(
                width, height, self.target_fps, self.target_bitrate_kbps
            )
            self.encoder_selected.emit(encoder_label)
        except Exception as e:
            self.error_occurred.emit(str(e))
            camera.stop()
            return

        frames_since_report = 0
        bytes_since_report = 0
        encode_time_total = 0.0
        capture_time_total = 0.0
        emit_time_total = 0.0
        last_report_time = time.time()
        pending_frame = first_frame

        try:
            while self._running:
                try:
                    conn, addr = self._server_socket.accept()
                    conn.setblocking(True)
                    self._clients.append(conn)
                    self.client_connected.emit()
                except BlockingIOError:
                    pass
                except OSError:
                    pass

                frame_start = time.time()

                if pending_frame is not None:
                    frame = pending_frame
                    pending_frame = None
                    t_capture = 0.0
                else:
                    t_capture_start = time.time()

                    if self.target_hwnd is not None:
                        current_index = get_current_monitor_index(self.target_hwnd, self.mss_monitors)
                        if (current_index is not None and current_index != self.monitor_index
                                and current_index in self.monitor_mapping):
                            camera.stop()
                            del camera
                            self.monitor_index = current_index
                            self.monitor = self.mss_monitors[current_index]
                            camera = dxcam.create(
                                output_idx=self.monitor_mapping[self.monitor_index], output_color="BGR"
                            )
                            current_region = self._current_region()
                            if current_region is None:
                                time.sleep(0.01)
                                continue
                            self._start_capture(camera, current_region)
                        else:
                            new_region = self._current_region()
                            if new_region is None:
                                time.sleep(0.01)
                                continue
                            if new_region != current_region:
                                current_region = new_region
                                camera.stop()
                                try:
                                    self._start_capture(camera, current_region)
                                except Exception:
                                    time.sleep(0.01)
                                    continue

                    raw = camera.get_latest_frame()
                    if raw is None:
                        time.sleep(0.001)
                        continue
                    frame = resize_frame(raw, self.target_height)
                    t_capture = time.time() - t_capture_start

                t_emit_start = time.time()
                self.frame_captured.emit(frame)
                t_emit = time.time() - t_emit_start

                t_encode_start = time.time()
                try:
                    packets = encode_frame(encoder_ctx, frame)
                except Exception as e:
                    self.error_occurred.emit(f"Erro ao codificar frame: {e}")
                    time.sleep(0.01)
                    continue
                t_encode = time.time() - t_encode_start

                encode_time_total += t_encode
                capture_time_total += t_capture
                emit_time_total += t_emit

                for packet in packets:
                    packet_bytes = bytes(packet)
                    bytes_since_report += len(packet_bytes)

                    if self._clients:
                        still_connected = []
                        for client_conn in self._clients:
                            try:
                                send_frame(client_conn, packet_bytes, timestamp=frame_start)
                                still_connected.append(client_conn)
                            except OSError:
                                try:
                                    client_conn.close()
                                except OSError:
                                    pass
                                self.client_disconnected.emit()
                        self._clients = still_connected

                frames_since_report += 1
                now = time.time()
                if now - last_report_time >= 1.0:
                    elapsed = now - last_report_time
                    fps = frames_since_report / elapsed
                    kbps = (bytes_since_report * 8 / 1024) / elapsed
                    avg_encode_ms = (encode_time_total * 1000) / frames_since_report
                    avg_capture_ms = (capture_time_total * 1000) / frames_since_report
                    avg_emit_ms = (emit_time_total * 1000) / frames_since_report
                    total_loop_ms = 1000 / fps if fps > 0 else 0
                    print(f"[STREAM] FPS: {fps:.1f} | loop total: {total_loop_ms:.1f}ms | "
                          f"captura: {avg_capture_ms:.1f}ms | emit preview: {avg_emit_ms:.1f}ms | "
                          f"encode: {avg_encode_ms:.1f}ms | kbps: {kbps:.0f}")
                    self.fps_updated.emit(fps)
                    self.stats_updated.emit(kbps)
                    frames_since_report = 0
                    bytes_since_report = 0
                    encode_time_total = 0.0
                    capture_time_total = 0.0
                    emit_time_total = 0.0
                    last_report_time = now

        except Exception as e:
            self.error_occurred.emit(str(e))
        finally:
            try:
                camera.stop()
            except Exception:
                pass
            for c in self._clients:
                try:
                    c.close()
                except OSError:
                    pass
            self._server_socket.close()

    def stop(self):
        self._running = False


class VideoReceiveThread(QThread):
    """Usado pelo Client: recebe o stream H.264 e decodifica continuamente.
    Reconecta automaticamente em segundo plano se a conexão cair."""

    frame_received = Signal(object, float, int)
    connection_lost = Signal()
    reconnected = Signal()

    def __init__(self, host_ip):
        super().__init__()
        self.host_ip = host_ip
        self._running = True
        self.socket = None
        self._ever_connected = False

    def run(self):
        decoder_ctx = create_decoder()

        while self._running:
            self.socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            self.socket.settimeout(2.0)
            try:
                self.socket.connect((self.host_ip, STREAM_PORT))
                self.socket.settimeout(None)
            except OSError:
                try:
                    self.socket.close()
                except OSError:
                    pass
                if not self._running:
                    break
                time.sleep(RECONNECT_RETRY_INTERVAL)
                continue

            if self._ever_connected:
                self.reconnected.emit()
            self._ever_connected = True

            while self._running:
                packet_bytes, timestamp = receive_frame(self.socket)
                if packet_bytes is None:
                    break
                latency = max(0.0, time.time() - timestamp)
                try:
                    frames = decode_packet(decoder_ctx, packet_bytes)
                except Exception:
                    continue
                for frame_ndarray in frames:
                    self.frame_received.emit(frame_ndarray, latency, len(packet_bytes))

            try:
                self.socket.close()
            except OSError:
                pass

            if self._running:
                self.connection_lost.emit()
                time.sleep(RECONNECT_RETRY_INTERVAL)

    def stop(self):
        self._running = False
        if self.socket:
            try:
                self.socket.close()
            except OSError:
                pass