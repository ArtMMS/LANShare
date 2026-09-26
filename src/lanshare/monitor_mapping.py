import time

import cv2
import dxcam
import mss
import numpy as np


def _small_gray(frame, size=(160, 90)):
    resized = cv2.resize(frame, size, interpolation=cv2.INTER_AREA)
    gray = cv2.cvtColor(resized, cv2.COLOR_BGR2GRAY)
    return gray.astype(np.float32)


def auto_map_monitors(mss_monitors):
    """Descobre automaticamente a correspondência entre os índices de monitor do mss
    (que sabemos a posição) e as saídas do dxcam (que não informam posição),
    comparando a similaridade visual de uma prévia de cada um. Não pede nada ao usuário."""

    real_monitors = [(i, m) for i, m in enumerate(mss_monitors) if i != 0]

    mss_previews = {}
    with mss.mss() as sct:
        for i, monitor in real_monitors:
            shot = sct.grab(monitor)
            frame = np.array(shot, dtype=np.uint8)[:, :, :3]
            mss_previews[i] = _small_gray(frame)

    dxcam_previews = {}
    output_count = len([line for line in dxcam.output_info().strip().split("\n") if line.strip()])
    for output_idx in range(output_count):
        camera = dxcam.create(output_idx=output_idx, output_color="BGR")
        frame = None
        for _ in range(20):
            frame = camera.grab()
            if frame is not None:
                break
            time.sleep(0.05)
        del camera
        if frame is not None:
            dxcam_previews[output_idx] = _small_gray(frame)

    mapping = {}
    used_outputs = set()
    for mss_idx, mss_img in mss_previews.items():
        best_output, best_diff = None, None
        for output_idx, dx_img in dxcam_previews.items():
            if output_idx in used_outputs:
                continue
            if mss_img.shape != dx_img.shape:
                continue
            diff = float(np.mean(np.abs(mss_img - dx_img)))
            if best_diff is None or diff < best_diff:
                best_diff, best_output = diff, output_idx
        if best_output is not None:
            mapping[mss_idx] = best_output
            used_outputs.add(best_output)

    return mapping