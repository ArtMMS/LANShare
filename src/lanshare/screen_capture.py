import ctypes

import cv2
import mss
import numpy as np

JPEG_QUALITY = 70


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


def get_cursor_position():
    pt = POINT()
    ctypes.windll.user32.GetCursorPos(ctypes.byref(pt))
    return pt.x, pt.y


def draw_cursor(frame, monitor):
    cursor_x, cursor_y = get_cursor_position()
    x = cursor_x - monitor["left"]
    y = cursor_y - monitor["top"]

    height, width = frame.shape[:2]
    if not (0 <= x < width and 0 <= y < height):
        return

    points = np.array([
        [x, y],
        [x, y + 16],
        [x + 4, y + 12],
        [x + 7, y + 19],
        [x + 9, y + 18],
        [x + 6, y + 11],
        [x + 11, y + 11],
    ], dtype=np.int32)

    cv2.fillPoly(frame, [points], color=(255, 255, 255))
    cv2.polylines(frame, [points], isClosed=True, color=(0, 0, 0), thickness=1)


def choose_monitor(sct):
    """Continua usando mss só para listar monitores e saber a posição (left/top) de cada um."""
    monitors = sct.monitors

    print("\nMonitores disponíveis:")
    for i, monitor in enumerate(monitors):
        if i == 0:
            continue
        largura = monitor["width"]
        altura = monitor["height"]
        print(f"  [{i}] Monitor {i} — {largura}x{altura}")

    while True:
        escolha = input("\nDigite o número do monitor que deseja capturar: ").strip()
        if escolha.isdigit() and 1 <= int(escolha) < len(monitors):
            return int(escolha), monitors[int(escolha)]
        print("Opção inválida, tente novamente.")


def compress_frame(frame, verbose=False):
    """Recebe um frame já capturado (array BGR) e devolve os bytes comprimidos em JPEG."""
    success, encoded = cv2.imencode(".jpg", frame, [cv2.IMWRITE_JPEG_QUALITY, JPEG_QUALITY])
    compressed_bytes = encoded.tobytes()

    if verbose:
        raw_size = frame.nbytes
        compressed_size = len(compressed_bytes)
        reducao = 100 - (compressed_size / raw_size * 100)
        print(f"[CAPTURE] Tamanho cru: {raw_size / 1024:.1f} KB")
        print(f"[CAPTURE] Tamanho comprimido (JPEG): {compressed_size / 1024:.1f} KB")
        print(f"[CAPTURE] Redução: {reducao:.1f}%")

    return compressed_bytes


def capture_screen(output_path="screenshot.jpg"):
    """Teste isolado de um único print — continua usando mss, simples o bastante para não precisar do dxcam aqui."""
    with mss.mss() as sct:
        _, monitor = choose_monitor(sct)
        screenshot = sct.grab(monitor)
        frame = np.ascontiguousarray(np.array(screenshot, dtype=np.uint8)[:, :, :3])
        draw_cursor(frame, monitor)
        compressed_bytes = compress_frame(frame, verbose=True)

        with open(output_path, "wb") as f:
            f.write(compressed_bytes)

        print(f"[CAPTURE] Screenshot salva em: {output_path}")


if __name__ == "__main__":
    capture_screen()