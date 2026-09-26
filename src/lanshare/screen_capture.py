import mss
import mss.tools


def choose_monitor(sct):
    monitors = sct.monitors  # índice 0 = todos os monitores juntos; 1, 2, 3... = monitores individuais

    print("\nMonitores disponíveis:")
    for i, monitor in enumerate(monitors):
        if i == 0:
            continue  # pula o "todos juntos", não é uma opção útil pro usuário escolher
        largura = monitor["width"]
        altura = monitor["height"]
        print(f"  [{i}] Monitor {i} — {largura}x{altura}")

    while True:
        escolha = input("\nDigite o número do monitor que deseja capturar: ").strip()
        if escolha.isdigit() and 1 <= int(escolha) < len(monitors):
            return monitors[int(escolha)]
        print("Opção inválida, tente novamente.")


def capture_screen(output_path="screenshot.png"):
    with mss.mss() as sct:
        monitor = choose_monitor(sct)
        screenshot = sct.grab(monitor)
        mss.tools.to_png(screenshot.rgb, screenshot.size, output=output_path)
        print(f"[CAPTURE] Screenshot salva em: {output_path}")


if __name__ == "__main__":
    capture_screen()