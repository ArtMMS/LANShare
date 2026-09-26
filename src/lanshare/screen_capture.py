import cv2
import mss
import numpy as np

DEFAULT_JPEG_QUALITY = 70

RESOLUTION_SCALES = {"100%": 1.0, "75%": 0.75, "50%": 0.5}
BITRATE_PRESETS = {"Baixo (~1500 kbps)": 1500, "Médio (~4000 kbps)": 4000, "Alto (~8000 kbps)": 8000}


def list_monitors():
    """Retorna [(index, monitor_dict), ...] só dos monitores reais (ignora o índice 0)."""
    with mss.mss() as sct:
        return [(i, m) for i, m in enumerate(sct.monitors) if i != 0]


def capture_monitor_preview(monitor):
    """Tira um print rápido do monitor via mss, para exibir como miniatura na interface.
    Retorna um array numpy BGR."""
    with mss.mss() as sct:
        shot = sct.grab(monitor)
        return np.ascontiguousarray(np.array(shot, dtype=np.uint8)[:, :, :3])


def resize_frame(frame, scale):
    if scale >= 1.0:
        return frame
    height, width = frame.shape[:2]
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