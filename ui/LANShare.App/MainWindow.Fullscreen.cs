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

        // Tunnel: a janela vê o F11/Esc antes dos campos de texto
        AddHandler(KeyDownEvent, OnFullscreenKeyDown, RoutingStrategies.Tunnel);
    }

    private void OnFullscreenKeyDown(object? sender, KeyEventArgs e)
    {
        if (e.Key == Key.F11)
        {
            ToggleFullscreen();
            e.Handled = true;
        }
        else if (e.Key == Key.Escape && _fullscreen)
        {
            SetFullscreen(false);
            e.Handled = true;
        }
    }

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