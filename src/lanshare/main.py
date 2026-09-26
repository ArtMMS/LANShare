import sys

import mss
from PySide6.QtWidgets import QApplication

from screen_capture import choose_monitor
from window_selector import choose_window
from monitor_mapping import calibrate_monitor_mapping
from gui import HostWindow, ClientWindow


def main():
    role = input("Você é [1] Host ou [2] Client? ").strip()

    if role == "1":
        username = input("Digite seu nome de usuário: ").strip() or "Host"

        with mss.mss() as sct:
            mss_monitors = sct.monitors
            monitor_index, monitor = choose_monitor(sct)

        target_hwnd = choose_window(monitor)
        monitor_mapping = calibrate_monitor_mapping(mss_monitors)

        app = QApplication(sys.argv)
        window = HostWindow(username, monitor_index, monitor, target_hwnd, monitor_mapping, mss_monitors)
        window.show()
        sys.exit(app.exec())

    elif role == "2":
        host_ip = input("Digite o IP do Host: ").strip()
        username = input("Digite seu nome de usuário: ").strip() or "Client"

        app = QApplication(sys.argv)
        window = ClientWindow(host_ip, username)
        window.show()
        sys.exit(app.exec())

    else:
        print("Opção inválida.")


if __name__ == "__main__":
    main()