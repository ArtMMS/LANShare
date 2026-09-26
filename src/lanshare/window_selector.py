import win32gui


def list_windows():
    """Retorna uma lista de (hwnd, título) de janelas visíveis e com título, ignorando janelas do sistema."""
    windows = []

    def enum_handler(hwnd, _):
        if win32gui.IsWindowVisible(hwnd):
            title = win32gui.GetWindowText(hwnd)
            if title.strip():
                windows.append((hwnd, title))

    win32gui.EnumWindows(enum_handler, None)
    return windows


def window_is_in_monitor(hwnd, monitor):
    """Verifica se a janela tem alguma sobreposição real com o monitor informado."""
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)
    mon_left = monitor["left"]
    mon_top = monitor["top"]
    mon_right = monitor["left"] + monitor["width"]
    mon_bottom = monitor["top"] + monitor["height"]

    # não há sobreposição se a janela está totalmente fora dos limites do monitor
    if right <= mon_left or left >= mon_right or bottom <= mon_top or top >= mon_bottom:
        return False
    return True


def choose_window(monitor):
    """Deixa o usuário escolher compartilhar o monitor inteiro ou uma janela específica,
    listando só janelas que estão de fato dentro do monitor escolhido."""
    all_windows = list_windows()
    windows = [(hwnd, title) for hwnd, title in all_windows if window_is_in_monitor(hwnd, monitor)]

    print("\nO que deseja compartilhar?")
    print("  [0] Monitor inteiro")
    for i, (hwnd, title) in enumerate(windows, start=1):
        print(f"  [{i}] {title}")

    while True:
        escolha = input("\nDigite o número da opção: ").strip()
        if escolha == "0":
            return None
        if escolha.isdigit() and 1 <= int(escolha) <= len(windows):
            return windows[int(escolha) - 1][0]
        print("Opção inválida, tente novamente.")


def get_window_region(hwnd, monitor):
    """Retorna o retângulo da janela (left, top, right, bottom) relativo ao monitor,
    recortado para os limites do monitor e ajustado para dimensões pares
    (exigência de várias APIs de captura de vídeo)."""
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)

    rel_left = max(0, left - monitor["left"])
    rel_top = max(0, top - monitor["top"])
    rel_right = min(monitor["width"], right - monitor["left"])
    rel_bottom = min(monitor["height"], bottom - monitor["top"])

    # garante que a região é válida (largura e altura positivas)
    if rel_right <= rel_left or rel_bottom <= rel_top:
        return None

    # arredonda para dimensões pares, cortando 1px se necessário
    if (rel_right - rel_left) % 2 != 0:
        rel_right -= 1
    if (rel_bottom - rel_top) % 2 != 0:
        rel_bottom -= 1

    if rel_right <= rel_left or rel_bottom <= rel_top:
        return None

    return (rel_left, rel_top, rel_right, rel_bottom)