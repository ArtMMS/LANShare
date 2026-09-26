import dxcam
import mss

print("=== Monitores segundo o mss ===")
with mss.mss() as sct:
    for i, monitor in enumerate(sct.monitors):
        print(f"[{i}] left={monitor['left']} top={monitor['top']} "
              f"width={monitor['width']} height={monitor['height']}")

print("\n=== Saídas (outputs) segundo o dxcam ===")
print(dxcam.output_info())
