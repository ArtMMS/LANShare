import ctypes

import cv2
import mss
import numpy as np

JPEG_QUALITY = 70


class POINT(ctypes.Structure):
    _fields_ = [("x", ctypes.c_long), ("y", ctypes.c_long)]


def get_cursor_position():
    """Retorna a posição absoluta do cursor na tela (coordenadas globais do Windows)."""
    pt = POINT()
    ctypes.windll.user32.GetCursorPos(ctypes.byref(pt))
    return pt.x, pt.y


def draw_cursor(frame, monitor):
    """Desenha um ponteiro de mouse sintético na posição atual do cursor, sobre o frame capturado."""
    cursor_x, cursor_y = get_cursor_position()

    # converte de coordenada absoluta da tela para coordenada relativa a este monitor/frame
    x = cursor_x - monitor["left"]
    y = cursor_y - monitor["top"]

    height, width = frame.shape[:2]
    if not (0 <= x < width and 0 <= y < height):
        return  # cursor está fora deste monitor, não desenha nada

    # formato de seta simples (parecido com o cursor padrão do Windows)
    points = np.array([
        [x, y],
        [x, y + 16],
        [x + 4, y + 12],
        [x + 7, y + 19],
        [x + 9, y + 18],
        [x + 6, y + 11],
        [x + 11, y + 11],
    ], dtype=np.int32)

    cv2.fillPoly(frame, [points], color=(255, 255, 255))  # preenchimento branco
    cv2.polylines(frame, [points], isClosed=True, color=(0, 0, 0), thickness=1)  # contorno preto


def choose_monitor(sct):
    monitors = sct.monitors  # índice 0 = todos os monitores juntos; 1, 2, 3... = monitores individuais

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
            return monitors[int(escolha)]
        print("Opção inválida, tente novamente.")


def capture_and_compress(monitor, sct, verbose=False):
    screenshot = sct.grab(monitor)

    # mss entrega em BGRA; convertendo para array numpy e descartando o canal alpha,
    # já ficamos em BGR — formato nativo do OpenCV, sem nenhuma conversão de cor extra
    frame = np.ascontiguousarray(np.array(screenshot, dtype=np.uint8)[:, :, :3])

    draw_cursor(frame, monitor)

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
    with mss.mss() as sct:
        monitor = choose_monitor(sct)
        compressed_bytes = capture_and_compress(monitor, sct, verbose=True)

        with open(output_path, "wb") as f:
            f.write(compressed_bytes)

        print(f"[CAPTURE] Screenshot salva em: {output_path}")


if __name__ == "__main__":
    capture_screen()