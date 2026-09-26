import time


class BitrateController:
    """Ajusta a qualidade do JPEG dinamicamente para tentar manter o bitrate
    real perto de uma meta (target_kbps), sem precisar de um valor fixo."""

    MIN_QUALITY = 20
    MAX_QUALITY = 90
    ADJUST_STEP = 5

    def __init__(self, target_kbps, initial_quality=70):
        self.target_kbps = target_kbps
        self.quality = initial_quality
        self._bytes_since_check = 0
        self._last_check_time = time.time()

    def register_frame(self, frame_bytes_len):
        self._bytes_since_check += frame_bytes_len

        now = time.time()
        elapsed = now - self._last_check_time
        if elapsed < 1.0:
            return self.quality  # só reavalia a cada 1 segundo real

        current_kbps = (self._bytes_since_check * 8 / 1024) / elapsed

        if current_kbps > self.target_kbps * 1.1:
            self.quality = max(self.MIN_QUALITY, self.quality - self.ADJUST_STEP)
        elif current_kbps < self.target_kbps * 0.8:
            self.quality = min(self.MAX_QUALITY, self.quality + self.ADJUST_STEP)

        self._bytes_since_check = 0
        self._last_check_time = now

        return self.quality

    def current_bitrate_kbps(self):
        elapsed = time.time() - self._last_check_time
        if elapsed <= 0:
            return 0.0
        return (self._bytes_since_check * 8 / 1024) / elapsed