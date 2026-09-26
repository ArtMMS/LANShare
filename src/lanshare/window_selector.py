import win32api
import win32con
import win32gui


def list_windows():
    windows = []

    def enum_handler(hwnd, _):
        if win32gui.IsWindowVisible(hwnd):
            title = win32gui.GetWindowText(hwnd)
            if title.strip():
                windows.append((hwnd, title))

    win32gui.EnumWindows(enum_handler, None)
    return windows


def window_is_in_monitor(hwnd, monitor):
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)
    mon_left, mon_top = monitor["left"], monitor["top"]
    mon_right = monitor["left"] + monitor["width"]
    mon_bottom = monitor["top"] + monitor["height"]

    if right <= mon_left or left >= mon_right or bottom <= mon_top or top >= mon_bottom:
        return False
    return True


def windows_in_monitor(monitor):
    return [(hwnd, title) for hwnd, title in list_windows() if window_is_in_monitor(hwnd, monitor)]


def get_window_region(hwnd, monitor):
    left, top, right, bottom = win32gui.GetWindowRect(hwnd)

    rel_left = max(0, left - monitor["left"])
    rel_top = max(0, top - monitor["top"])
    rel_right = min(monitor["width"], right - monitor["left"])
    rel_bottom = min(monitor["height"], bottom - monitor["top"])

    if rel_right <= rel_left or rel_bottom <= rel_top:
        return None

    if (rel_right - rel_left) % 2 != 0:
        rel_right -= 1
    if (rel_bottom - rel_top) % 2 != 0:
        rel_bottom -= 1

    if rel_right <= rel_left or rel_bottom <= rel_top:
        return None

    return (rel_left, rel_top, rel_right, rel_bottom)


def get_current_monitor_index(hwnd, mss_monitors):
    try:
        hmonitor = win32api.MonitorFromWindow(hwnd, win32con.MONITOR_DEFAULTTONEAREST)
        info = win32api.GetMonitorInfo(hmonitor)
        win_left, win_top = info["Monitor"][0], info["Monitor"][1]
    except Exception:
        return None

    for i, monitor in enumerate(mss_monitors):
        if i == 0:
            continue
        if monitor["left"] == win_left and monitor["top"] == win_top:
            return i
    return None