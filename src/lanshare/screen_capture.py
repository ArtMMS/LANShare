import cv2
import mss
import numpy as np

JPEG_QUALITY = 70


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
        compressed_bytes = compress_frame(frame, verbose=True)

        with open(output_path, "wb") as f:
            f.write(compressed_bytes)

        print(f"[CAPTURE] Screenshot salva em: {output_path}")


if __name__ == "__main__":
    capture_screen()