using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;

namespace LANShare.App;

/// Volume do som da transmissão (só no Client): botão no canto inferior esquerdo do vídeo
/// e, ao clicar, um painel com o slider vertical e o mudo. O volume fica salvo nas preferências.
/// Em tela cheia o botão (e o cursor) somem quando o mouse fica parado, como num player de vídeo.
public partial class MainWindow
{
    /// Em tela cheia, depois deste tempo sem mexer o mouse, o botão e o cursor somem.
    private static readonly TimeSpan VolumeAutoHide = TimeSpan.FromSeconds(2.5);

    /// O volume é salvo um pouco depois da última mudança (arrastar o slider gera muitas).
    private static readonly TimeSpan VolumeSaveDelay = TimeSpan.FromMilliseconds(600);

    /// Quanto a roda do mouse muda o volume a cada "clique".
    private const int VolumeWheelStep = 5;

    private const double VolumeButtonSize = 44;
    private const double VolumeEdgeMargin = 16;

    private const string VolumeMutedGlyph = "";
    private const string Volume0Glyph = "";
    private const string Volume1Glyph = "";
    private const string Volume2Glyph = "";
    private const string Volume3Glyph = "";

    private static readonly IBrush VolumeBackground = new SolidColorBrush(Color.Parse("#E01B1F24"));
    private static readonly IBrush VolumeBackgroundHover = new SolidColorBrush(Color.Parse("#F02A3038"));
    private static readonly IBrush VolumeBorder = new SolidColorBrush(Color.Parse("#3A424C"));

    private Border _volumeButton = null!, _volumePanel = null!, _volumeMuteButton = null!;
    private TextBlock _volumeButtonIcon = null!, _volumeMuteIcon = null!, _volumeLabel = null!;
    private Slider _volumeSlider = null!;

    private readonly DispatcherTimer _volumeHideTimer = new();
    private readonly DispatcherTimer _volumeSaveTimer = new();

    private bool _volumePanelOpen;
    private bool _volumeHiddenByIdle;
    private bool _updatingVolumeSlider;

    /// O botão de volume faz sentido agora? (Client conectado.)
    private bool VolumeAvailable => _mode == Mode.Client && _clientState == ClientState.Connected;

    /// Chamado uma vez no construtor (antes do InitFullscreen).
    private void InitVolume()
    {
        _volumeButtonIcon = VolumeGlyph(20);
        _volumeButton = new Border
        {
            Width = VolumeButtonSize,
            Height = VolumeButtonSize,
            CornerRadius = new CornerRadius(8),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            Margin = new Thickness(VolumeEdgeMargin),
            Cursor = new Cursor(StandardCursorType.Hand),
            BoxShadow = BoxShadows.Parse("0 4 14 0 #80000000"),
            ZIndex = 20,
            IsVisible = false,
            Child = _volumeButtonIcon,
        };
        ToolTip.SetTip(_volumeButton, Loc.T("Volume_Tooltip"));

        _volumeLabel = new TextBlock
        {
            FontSize = 12,
            HorizontalAlignment = HorizontalAlignment.Center,
            Opacity = 0.9,
        };

        _volumeSlider = new Slider
        {
            Orientation = Orientation.Vertical,
            Minimum = 0,
            Maximum = 100,
            SmallChange = 1,
            LargeChange = 10,
            Height = 140,
            HorizontalAlignment = HorizontalAlignment.Center,
        };

        _volumeMuteIcon = VolumeGlyph(16);
        _volumeMuteButton = new Border
        {
            Width = 32,
            Height = 32,
            CornerRadius = new CornerRadius(6),
            Background = Brushes.Transparent,
            HorizontalAlignment = HorizontalAlignment.Center,
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = _volumeMuteIcon,
        };

        _volumePanel = new Border
        {
            Width = 64,
            CornerRadius = new CornerRadius(10),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(6, 10),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            // Logo acima do botão, alinhado com ele
            Margin = new Thickness(
                VolumeEdgeMargin - (64 - VolumeButtonSize) / 2,
                0,
                0,
                VolumeEdgeMargin + VolumeButtonSize + 8),
            BoxShadow = BoxShadows.Parse("0 8 24 0 #80000000"),
            ZIndex = 21,
            IsVisible = false,
            Child = new StackPanel
            {
                Spacing = 8,
                Children = { _volumeLabel, _volumeSlider, _volumeMuteButton },
            },
        };

        // Por cima do vídeo (mesmo Grid da imagem)
        if (_previewImage.Parent is Panel stage)
        {
            stage.Children.Add(_volumeButton);
            stage.Children.Add(_volumePanel);
        }

        // Botão: clicar abre/fecha o painel; passar o mouse clareia
        _volumeButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_volumeButton).Properties.IsLeftButtonPressed) return;
            SetVolumePanel(!_volumePanelOpen);
            e.Handled = true;
        };
        _volumeButton.PointerEntered += (_, _) => _volumeButton.Background = VolumeBackgroundHover;
        _volumeButton.PointerExited += (_, _) => _volumeButton.Background = VolumeBackground;

        // Mudo
        _volumeMuteButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_volumeMuteButton).Properties.IsLeftButtonPressed) return;
            SetVolume(_preferences.StreamVolume, !_preferences.StreamMuted);
            e.Handled = true;
        };
        _volumeMuteButton.PointerEntered += (_, _) => _volumeMuteButton.Background = VolumeBackgroundHover;
        _volumeMuteButton.PointerExited += (_, _) => _volumeMuteButton.Background = Brushes.Transparent;

        // Arrastar o slider tira do mudo
        _volumeSlider.ValueChanged += (_, _) =>
        {
            if (_updatingVolumeSlider) return;
            SetVolume((int)Math.Round(_volumeSlider.Value), muted: false);
        };

        // Roda do mouse sobre o botão ou o painel
        _volumeButton.PointerWheelChanged += OnVolumeWheel;
        _volumePanel.PointerWheelChanged += OnVolumeWheel;

        // Clicar fora do botão e do painel fecha o painel
        AddHandler(PointerPressedEvent, (_, e) =>
        {
            if (!_volumePanelOpen || e.Source is not Visual source) return;
            if (IsInside(source, _volumePanel) || IsInside(source, _volumeButton)) return;
            SetVolumePanel(false);
        }, RoutingStrategies.Tunnel);

        // Esc fecha o painel (antes de a tela cheia ver a tecla)
        AddHandler(KeyDownEvent, (_, e) =>
        {
            if (!_volumePanelOpen || e.Key != Key.Escape) return;
            SetVolumePanel(false);
            e.Handled = true;
        }, RoutingStrategies.Tunnel);

        // Tela cheia: mexer o mouse mostra o botão de novo
        AddHandler(PointerMovedEvent, (_, _) => OnVolumeMouseActivity(), RoutingStrategies.Tunnel, handledEventsToo: true);

        _volumeHideTimer.Interval = VolumeAutoHide;
        _volumeHideTimer.Tick += (_, _) => HideVolumeForIdle();

        _volumeSaveTimer.Interval = VolumeSaveDelay;
        _volumeSaveTimer.Tick += (_, _) =>
        {
            _volumeSaveTimer.Stop();
            _preferences.Save();
        };

        // Salva o que ainda estiver pendente ao fechar o app
        Closing += (_, _) =>
        {
            if (!_volumeSaveTimer.IsEnabled) return;
            _volumeSaveTimer.Stop();
            _preferences.Save();
        };

        RefreshVolumeVisuals();
    }

    /// Manda o volume salvo para o motor (ao entrar no modo Client).
    private void ApplyVolume()
    {
        if (_clientActive) NativeMethods.ClientSetVolume(_preferences.StreamVolume, _preferences.StreamMuted);
    }

    /// Muda o volume: aplica na hora, atualiza o visual e salva logo depois.
    private void SetVolume(int percent, bool muted)
    {
        percent = Math.Clamp(percent, 0, 100);
        if (percent == _preferences.StreamVolume && muted == _preferences.StreamMuted) return;

        _preferences.StreamVolume = percent;
        _preferences.StreamMuted = muted;
        ApplyVolume();
        RefreshVolumeVisuals();

        _volumeSaveTimer.Stop();
        _volumeSaveTimer.Start();
    }

    private void OnVolumeWheel(object? sender, PointerWheelEventArgs e)
    {
        var step = e.Delta.Y > 0 ? VolumeWheelStep : e.Delta.Y < 0 ? -VolumeWheelStep : 0;
        if (step == 0) return;

        // Aumentar com a roda tira do mudo; diminuir no mudo continua no mudo
        var muted = _preferences.StreamMuted && step < 0;
        SetVolume(_preferences.StreamVolume + step, muted);
        e.Handled = true;
    }

    /// Ícone, texto e slider de acordo com o volume atual.
    private void RefreshVolumeVisuals()
    {
        var volume = _preferences.StreamVolume;
        var muted = _preferences.StreamMuted;

        var glyph = muted || volume == 0 ? (muted ? VolumeMutedGlyph : Volume0Glyph)
            : volume < 34 ? Volume1Glyph
            : volume < 67 ? Volume2Glyph
            : Volume3Glyph;

        _volumeButtonIcon.Text = glyph;
        _volumeMuteIcon.Text = muted ? VolumeMutedGlyph : glyph;
        _volumeLabel.Text = muted ? "Vol 0%" : $"Vol {volume}%";
        ToolTip.SetTip(_volumeMuteButton, Loc.T(muted ? "Volume_Unmute" : "Volume_Mute"));

        // No mudo o slider mostra onde o volume vai voltar, mas apagado
        _updatingVolumeSlider = true;
        _volumeSlider.Value = volume;
        _updatingVolumeSlider = false;
        _volumeSlider.Opacity = muted ? 0.45 : 1;
    }

    /// Mostra ou esconde o botão conforme a conexão (chamado pelo UpdateClientUi e pelo ShowMode).
    private void UpdateVolumeControls()
    {
        if (!VolumeAvailable)
        {
            SetVolumePanel(false);
            _volumeButton.IsVisible = false;
            _volumeHideTimer.Stop();
            RestoreCursor();
        }
        else
        {
            _volumeButton.IsVisible = !_volumeHiddenByIdle;
            if (_fullscreen) _volumeHideTimer.Start();
        }

        // A tela cheia também só vale com o Client conectado
        UpdateFullscreenAvailability();
    }

    private void SetVolumePanel(bool open)
    {
        if (open && !VolumeAvailable) open = false;
        _volumePanelOpen = open;
        _volumePanel.IsVisible = open;

        // Painel fechado em tela cheia: volta a contar o tempo para esconder o botão
        if (!open && _fullscreen && VolumeAvailable)
        {
            _volumeHideTimer.Stop();
            _volumeHideTimer.Start();
        }
    }

    /// Chamado pelo SetFullscreen.
    private void OnVolumeFullscreenChanged(bool fullscreen)
    {
        _volumeHiddenByIdle = false;
        RestoreCursor();

        if (fullscreen && VolumeAvailable)
        {
            _volumeButton.IsVisible = true;
            _volumeHideTimer.Stop();
            _volumeHideTimer.Start();
        }
        else
        {
            _volumeHideTimer.Stop();
            _volumeButton.IsVisible = VolumeAvailable;
        }
    }

    /// O mouse mexeu: em tela cheia, mostra o botão e o cursor de novo e recomeça a contagem.
    private void OnVolumeMouseActivity()
    {
        if (!_fullscreen || !VolumeAvailable) return;

        if (_volumeHiddenByIdle)
        {
            _volumeHiddenByIdle = false;
            _volumeButton.IsVisible = true;
            RestoreCursor();
        }

        _volumeHideTimer.Stop();
        _volumeHideTimer.Start();
    }

    /// Tela cheia sem mexer o mouse: esconde o botão e o cursor (só se o painel estiver fechado).
    private void HideVolumeForIdle()
    {
        _volumeHideTimer.Stop();
        if (!_fullscreen || !VolumeAvailable || _volumePanelOpen) return;
        if (_volumeButton.IsPointerOver) return; // o mouse está parado em cima do botão

        _volumeHiddenByIdle = true;
        _volumeButton.IsVisible = false;
        Cursor = new Cursor(StandardCursorType.None);
    }

    private void RestoreCursor() => Cursor = Cursor.Default;

    private static TextBlock VolumeGlyph(double size) => new()
    {
        FontFamily = IconFont,
        FontSize = size,
        HorizontalAlignment = HorizontalAlignment.Center,
        VerticalAlignment = VerticalAlignment.Center,
    };

    /// `source` está dentro de `container` (ou é ele)?
    private static bool IsInside(Visual source, Visual container)
    {
        for (Visual? current = source; current is not null; current = current.GetVisualParent())
        {
            if (ReferenceEquals(current, container)) return true;
        }
        return false;
    }
}
