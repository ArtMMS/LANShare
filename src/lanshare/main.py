import sys

from PySide6.QtWidgets import QApplication

from gui import LauncherWindow


def main():
    app = QApplication(sys.argv)
    launcher = LauncherWindow()
    launcher.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()