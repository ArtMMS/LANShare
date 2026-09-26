import io
import mss
import mss.tools
from PIL import Image

JPEG_QUALITY = 70


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
            return monitors[int(escolha)]
        print("Opção inválida, tente novamente.")


def capture_and_compress(monitor, sct, verbose=False):
    screenshot = sct.grab(monitor)

    img = Image.frombytes("RGB", screenshot.size, screenshot.rgb)

    buffer = io.BytesIO()
    img.save(buffer, format="JPEG", quality=JPEG_QUALITY)
    compressed_bytes = buffer.getvalue()

    if verbose:
        raw_size = len(screenshot.rgb)
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