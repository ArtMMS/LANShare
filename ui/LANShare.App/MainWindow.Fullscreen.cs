using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;

namespace LANShare.App;

public partial class MainWindow
{
    private Border _sidebar = null!;
    private bool _fullscreen;
    private WindowState _stateBeforeFullscreen = WindowState.Normal;

    /// Chamado uma vez no construtor, depois do InitializeComponent.
    private void InitFullscreen()
    {
        _sidebar = Find<Border>("Sidebar");

        // Tunnel: a janela vê a tecla antes dos campos de texto
        AddHandler(KeyDownEvent, OnFullscreenKeyDown, RoutingStrategies.Tunnel);

        // O atalho da tela cheia (e o resto das preferências) vem das configurações
        InitSettings();
    }

    private void OnFullscreenKeyDown(object? sender, KeyEventArgs e)
    {
        // Com as configurações abertas, a tecla pode estar sendo gravada como atalho novo
        if (_settingsOpen) return;

        if (_preferences.FullscreenKeyValue is { } shortcut && e.Key == shortcut)
        {
            // Digitando num campo, só teclas F1-F24 valem (uma letra continua sendo letra)
            if (FocusManager?.GetFocusedElement() is TextBox && !IsFunctionKey(shortcut)) return;

            ToggleFullscreen();
            e.Handled = true;
        }
        else if (e.Key == Key.Escape && _fullscreen)
        {
            SetFullscreen(false);
            e.Handled = true;
        }
    }

    private static bool IsFunctionKey(Key key) => key >= Key.F1 && key <= Key.F24;

    /// Só o Client conectado pode usar a tela cheia (no Host ela fica desativada).
    private bool FullscreenAvailable =>
        _mode == Mode.Client && _clientState == ClientState.Connected;

    private void ToggleFullscreen()
    {
        if (_fullscreen) SetFullscreen(false);
        else if (FullscreenAvailable) SetFullscreen(true);
    }

    /// Sai da tela cheia se ela deixou de fazer sentido (desconectado, expulso, voltou ao início...).
    private void UpdateFullscreenAvailability()
    {
        if (!FullscreenAvailable) SetFullscreen(false);
    }

    private void SetFullscreen(bool on)
    {
        if (on == _fullscreen) return;
        _fullscreen = on;

        if (on)
        {
            _stateBeforeFullscreen = WindowState == WindowState.FullScreen ? WindowState.Normal : WindowState;
            WindowState = WindowState.FullScreen;
        }
        else
        {
            WindowState = _stateBeforeFullscreen;
        }

        // Em tela cheia só o vídeo fica na tela
        _sidebar.IsVisible = !on;
        _mainLayout.ColumnDefinitions[0].Width = new GridLength(on ? 0 : 300);
        _infoText.IsVisible = !on;
        _previewImage.Margin = new Thickness(on ? 0 : 16);
    }
}