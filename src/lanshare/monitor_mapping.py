import time

import cv2
import dxcam


def _count_dxcam_outputs():
    info = dxcam.output_info()
    return len([line for line in info.strip().split("\n") if line.strip()])


def calibrate_monitor_mapping(mss_monitors):
    """Descobre a correspondência real entre os índices de monitor do mss e os
    output_idx do dxcam. O dxcam não informa a posição (left/top) de cada saída,
    só a ordem interna dele — que pode não bater com a do mss, especialmente
    com monitores de mesma resolução. Por isso, pedimos confirmação visual."""

    output_count = _count_dxcam_outputs()
    previews = []

    print("\n[CALIBRAÇÃO] Identificando monitores, aguarde...")
    for output_idx in range(output_count):
        camera = dxcam.create(output_idx=output_idx, output_color="BGR")
        frame = None
        for _ in range(20):
            frame = camera.grab()
            if frame is not None:
                break
            time.sleep(0.05)
        del camera

        if frame is None:
            print(f"[CALIBRAÇÃO] Não foi possível capturar a saída {output_idx}, pulando.")
            continue

        preview_path = f"monitor_preview_{output_idx}.jpg"
        small = cv2.resize(frame, (frame.shape[1] // 3, frame.shape[0] // 3))
        cv2.imwrite(preview_path, small)
        previews.append(output_idx)
        print(f"[CALIBRAÇÃO] Prévia salva: {preview_path} (output_idx={output_idx})")

    print("\nAbra essas imagens (na raiz do projeto) e identifique qual monitor cada uma mostra.")

    real_monitors = [(i, m) for i, m in enumerate(mss_monitors) if i != 0]
    for i, monitor in real_monitors:
        print(f"  Monitor mss [{i}]: posição left={monitor['left']} top={monitor['top']}, "
              f"{monitor['width']}x{monitor['height']}")

    mapping = {}  # mss_index -> dxcam_output_idx
    for i, monitor in real_monitors:
        while True:
            escolha = input(
                f"\nQual output_idx corresponde ao monitor mss [{i}] "
                f"(left={monitor['left']}, top={monitor['top']})? "
            ).strip()
            if escolha.isdigit() and int(escolha) in previews:
                mapping[i] = int(escolha)
                break
            print("Opção inválida, tente novamente.")

    return mapping