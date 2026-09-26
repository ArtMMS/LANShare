import cv2
import mss
import numpy as np

DEFAULT_JPEG_QUALITY = 70


def choose_monitor(sct):
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


def choose_resolution_scale():
    print("\nQualidade de resolução da transmissão:")
    print("  [1] 100% (nítido, mais dados)")
    print("  [2] 75%")
    print("  [3] 50% (mais leve, menos nítido)")

    options = {"1": 1.0, "2": 0.75, "3": 0.5}
    while True:
        escolha = input("Escolha uma opção: ").strip()
        if escolha in options:
            return options[escolha]
        print("Opção inválida, tente novamente.")


def choose_target_bitrate():
    print("\nMeta de bitrate da transmissão (controla o quanto de dados é usado por segundo):")
    print("  [1] Baixo (~1500 kbps) — prioriza economia de rede")
    print("  [2] Médio (~4000 kbps) — equilíbrio")
    print("  [3] Alto (~8000 kbps) — prioriza qualidade")

    options = {"1": 1500, "2": 4000, "3": 8000}
    while True:
        escolha = input("Escolha uma opção: ").strip()
        if escolha in options:
            return options[escolha]
        print("Opção inválida, tente novamente.")


def resize_frame(frame, scale):
    """Redimensiona o frame pela escala informada (1.0 = tamanho original)."""
    if scale >= 1.0:
        return frame
    height, width = frame.shape[:2]
    new_width = max(2, int(width * scale) // 2 * 2)   # garante dimensão par
    new_height = max(2, int(height * scale) // 2 * 2)
    return cv2.resize(frame, (new_width, new_height), interpolation=cv2.INTER_AREA)


def compress_frame(frame, quality=DEFAULT_JPEG_QUALITY, verbose=False):
    """Recebe um frame já capturado (array BGR) e devolve os bytes comprimidos em JPEG."""
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


def capture_screen(output_path="screenshot.jpg"):
    with mss.mss() as sct:
        _, monitor = choose_monitor(sct)
        screenshot = sct.grab(monitor)
        frame = np.ascontiguousarray(np.array(screenshot, dtype=np.uint8)[:, :, :3])
        compressed_bytes = compress_frame(frame, verbose=True)

        with open(output_path, "wb") as f:
            f.write(compressed_bytes)

        print(f"[CAPTURE] Screenshot salva em: {output_path}")


if __name__ == "__main__":
    capture_screen()