using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;

namespace LANShare.App;

/// Painel de configurações: abre pela engrenagem, por cima de tudo, com o fundo escurecido e desfocado.
public partial class MainWindow
{
    /// Preferências da interface (lidas uma vez ao abrir o app; o painel salva as mudanças).
    private readonly AppPreferences _preferences = AppPreferences.Load();

    private Border _settingsOverlay = null!;
    private SettingsView _settingsView = null!;
    private bool _settingsOpen;

    /// Chamado uma vez, pelo InitFullscreen (o atalho da tela cheia vem das preferências).
    private void InitSettings()
    {
        _settingsView = new SettingsView
        {
            HorizontalAlignment = HorizontalAlignment.Stretch,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(24),
        };
        _settingsView.CloseRequested += (_, _) => CloseSettings();
        _settingsView.PreferencesChanged += (_, _) => ApplyStreamDefaults();

        _settingsOverlay = new Border
        {
            Background = new SolidColorBrush(Color.Parse("#99000000")),
            IsVisible = false,
            ZIndex = 50,
            Child = _settingsView,
        };

        // Clicar na parte escurecida (fora do painel) fecha
        _settingsOverlay.PointerPressed += (_, e) =>
        {
            if (ReferenceEquals(e.Source, _settingsOverlay)) CloseSettings();
        };

        if (Content is Panel root) root.Children.Add(_settingsOverlay);

        var settingsButton = Find<Button>("SettingsButton");
        ToolTip.SetTip(settingsButton, "Configurações");
        settingsButton.Click += (_, _) => OpenSettings();

        AddHandler(KeyDownEvent, OnSettingsKeyDown, RoutingStrategies.Tunnel);

        ApplyStreamDefaults();
    }

    private void OpenSettings()
    {
        if (_settingsOpen || _pickerOpen || _avatarEditorOpen) return;

        _settingsOpen = true;
        _settingsView.Load(_preferences);
        _settingsOverlay.IsVisible = true;

        // Fundo travado e desfocado enquanto o painel está aberto
        _mainLayout.IsEnabled = false;
        _mainLayout.Effect = new BlurEffect { Radius = 12 };
    }

    private void CloseSettings()
    {
        if (!_settingsOpen) return;

        _settingsView.CancelCapture();
        _settingsOpen = false;
        _settingsOverlay.IsVisible = false;
        _mainLayout.IsEnabled = true;
        _mainLayout.Effect = null;
    }

    private void OnSettingsKeyDown(object? sender, KeyEventArgs e)
    {
        // Gravando a tecla do atalho: o Esc é dela ("sem atalho"), não fecha o painel
        if (!_settingsOpen || e.Key != Key.Escape || _settingsView.IsCapturingKey) return;
        CloseSettings();
        e.Handled = true;
    }

    /// Coloca a resolução e o FPS padrão nos seletores da lateral (só fora de uma transmissão).
    private void ApplyStreamDefaults()
    {
        if (_streaming) return;
        _resolutionBox.SelectedIndex = _preferences.StreamHeight == 720 ? 0 : 1;
        _fpsBox.SelectedIndex = _preferences.StreamFps == 15 ? 0 : 1;
    }

    /// O som desse tipo de aviso está ligado nas configurações?
    private bool SoundAllowed(Sound sound) => sound switch
    {
        Sound.StreamStarted => _preferences.SoundOnStreamStart,
        _ => _preferences.SoundOnUserJoinLeave,
    };
}