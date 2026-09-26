import cv2
import mss
import numpy as np

DEFAULT_JPEG_QUALITY = 70

# Resolução-alvo pela altura (em pixels), no estilo "qualidade de vídeo" (YouTube etc.)
RESOLUTION_PRESETS = {
    "720p (HD)": 720,
    "1080p (Full HD)": 1080,
    "1440p (Quad HD)": 1440,
    "2160p (4K)": 2160,
}

FPS_PRESETS = {
    "15 FPS": 15,
    "30 FPS": 30,
    "60 FPS": 60,
}

BITRATE_PRESETS = {"Baixo (~1500 kbps)": 1500, "Médio (~4000 kbps)": 4000, "Alto (~8000 kbps)": 8000}


def list_monitors():
    with mss.mss() as sct:
        return [(i, m) for i, m in enumerate(sct.monitors) if i != 0]


def capture_monitor_preview(monitor):
    with mss.mss() as sct:
        shot = sct.grab(monitor)
        return np.ascontiguousarray(np.array(shot, dtype=np.uint8)[:, :, :3])


def resize_frame(frame, target_height):
    """Redimensiona o frame para a altura-alvo, mantendo a proporção original.
    Nunca faz upscale: se o monitor já é menor que o alvo (ex: monitor 1080p
    com meta de 4K), mantém o tamanho nativo em vez de "esticar" a imagem."""
    height, width = frame.shape[:2]
    if target_height >= height:
        return frame
    scale = target_height / height
    new_width = max(2, int(width * scale) // 2 * 2)
    new_height = max(2, int(height * scale) // 2 * 2)
    return cv2.resize(frame, (new_width, new_height), interpolation=cv2.INTER_AREA)


def compress_frame(frame, quality=DEFAULT_JPEG_QUALITY, verbose=False):
    success, encoded = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, quality])
    compressed_bytes = encoded.tobytes()

    if verbose:
        raw_size = frame.nbytes
        compressed_size = len(compressed_bytes)
        reducao = 100 - (compressed_size / raw_size * 100)
        print(f"[CAPTURE] Tamanho cru: {raw_size / 1024:.1f} KB")
        print(f"[CAPTURE] Tamanho comprimido (JPEG): {compressed_size / 1024:.1f} KB")
        print(f"[CAPTURE] Redução: {reducao:.1f}%")

    return compressed_bytes