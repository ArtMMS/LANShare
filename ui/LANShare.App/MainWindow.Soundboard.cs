using Avalonia;
using Avalonia.Animation;
using Avalonia.Animation.Easings;
using Avalonia.Collections;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Transformation;
using Avalonia.Platform.Storage;
using Avalonia.Threading;

namespace LANShare.App;

/// Efeitos sonoros (soundboard), no Host com a sala aberta e no Client conectado:
/// - botão 🎉 na parte de baixo do vídeo; ao clicar, o painel com os sons (3 por fileira);
/// - ícone de volume no canto do painel: abre um slider deitado, por cima do painel;
/// - "Adicionar som" abre uma janela (como a do avatar) para escolher/arrastar o arquivo e dar um nome;
/// - quem toca um som fica com a borda azul e um 🎉 na lista de usuários, enquanto o som toca.
/// Por enquanto os sons tocam só neste PC (a rede vem na próxima etapa).
public partial class MainWindow
{
    private const double SfxButtonSize = 44;
    private const double SfxPanelWidth = 360;
    private const double SfxListMaxHeight = 196;
    private const double SfxTileHeight = 40;
    private const double SfxVolumeBoxWidth = 240;

    /// Espaço entre o painel e o slider de volume que abre por cima dele.
    private const double SfxVolumeGap = 8;

    /// Opacidade do botão parado (ele fica "escondido" até passar o mouse).
    private const double SfxIdleOpacity = 0.35;

    /// Quanto a roda do mouse muda o volume dos efeitos a cada "clique".
    private const int SfxWheelStep = 5;

    private const string SfxAudioGlyph = "";
    private const string SfxPlayGlyph = "";
    private const string PartyEmoji = "\U0001F389"; // 🎉

    private static readonly TimeSpan SfxHoverAnimation = TimeSpan.FromMilliseconds(160);
    private static readonly TimeSpan SfxErrorDuration = TimeSpan.FromSeconds(5);

    /// O slider do volume some este tempo depois de o mouse sair dele (se não estiver fixado).
    private static readonly TimeSpan SfxVolumeHideDelay = TimeSpan.FromMilliseconds(350);

    /// Destaque na lista de usuários: dura o tempo do som (com um mínimo, para dar tempo de ver).
    private static readonly TimeSpan SfxHighlightMin = TimeSpan.FromSeconds(1.5);
    private static readonly TimeSpan SfxHighlightExtra = TimeSpan.FromMilliseconds(400);

    private static readonly TransformOperations SfxScaleIdle = TransformOperations.Parse("scale(1)");
    private static readonly TransformOperations SfxScaleHot = TransformOperations.Parse("scale(1.12)");

    /// Nome dos sons: texto normal, com fallback para emoji colorido.
    private static readonly FontFamily SfxTextFont = new("Segoe UI, Segoe UI Emoji");
    private static readonly FontFamily SfxEmojiFont = new("Segoe UI Emoji");

    private static readonly IBrush SfxDashBrush = new SolidColorBrush(Color.Parse("#5A6470"));
    private static readonly IBrush SfxErrorBrush = new SolidColorBrush(Color.Parse("#FF6B6B"));
    private static readonly IBrush SfxHighlightBorder = new SolidColorBrush(Color.Parse("#4F7CFF"));
    private static readonly IBrush SfxHighlightFill = new SolidColorBrush(Color.Parse("#264F7CFF"));
    private static readonly IBrush SfxDialogBackground = new SolidColorBrush(Color.Parse("#1B1F24"));
    private static readonly IBrush SfxDialogBorder = new SolidColorBrush(Color.Parse("#2E343C"));
    private static readonly IBrush SfxDropBackground = new SolidColorBrush(Color.Parse("#15191E"));
    private static readonly IBrush SfxDropBorder = new SolidColorBrush(Color.Parse("#3A414B"));
    private static readonly IBrush SfxDropBorderHot = new SolidColorBrush(Color.Parse("#4F7CFF"));

    private static readonly FilePickerFileType SfxFileTypes = new(Loc.T("Sfx_FileType"))
    {
        Patterns = new[] { "*.mp3", "*.wav", "*.ogg" },
    };

    // ── Botão, painel e volume ──
    private Border _sfxButton = null!, _sfxPanel = null!, _sfxVolumeButton = null!, _sfxVolumeBox = null!,
        _sfxAddButton = null!;
    private TextBlock _sfxVolumeIcon = null!, _sfxVolumeLabel = null!, _sfxError = null!, _sfxEmptyText = null!;
    private Slider _sfxVolumeSlider = null!;
    private UniformGrid _sfxGrid = null!;
    private ScrollViewer _sfxScroll = null!;
    private Grid _sfxRenameRow = null!;
    private TextBox _sfxRenameBox = null!;

    // ── Janela "Adicionar som" ──
    private Border _sfxAddOverlay = null!, _sfxAddDrop = null!, _sfxAddFileCard = null!;
    private TextBlock _sfxAddDropTitle = null!, _sfxAddFileName = null!, _sfxAddFileInfo = null!, _sfxAddError = null!;
    private StackPanel _sfxAddNameSection = null!;
    private TextBox _sfxAddNameBox = null!;
    private Button _sfxAddChooseAnother = null!, _sfxAddSave = null!, _sfxAddPreview = null!;

    private readonly DispatcherTimer _sfxVolumeHideTimer = new();
    private IDisposable? _sfxErrorTimer;

    private List<SfxEntry> _sfxEntries = new();
    private bool _sfxPanelOpen;
    private bool _sfxVolumePinned;   // clicou no ícone: o slider fica aberto até clicar de novo
    private bool _sfxVolumeHeld;     // arrastando o slider (não esconde no meio)
    private bool _updatingSfxSlider;
    private string? _sfxRenamingId;

    // Estado da janela "Adicionar som": o arquivo escolhido e já conferido
    private bool _sfxAddOpen;
    private bool _sfxAddBusy;
    private byte[]? _sfxAddData;
    private SfxCheck? _sfxAddCheck;

    /// Quem está com o destaque azul na lista de usuários, e até quando (id 0 = o Host).
    private readonly Dictionary<ulong, DateTime> _sfxHighlights = new();

    /// O soundboard faz sentido agora? (Host com a sala aberta ou Client conectado.)
    private bool SoundboardAvailable =>
        (_mode == Mode.Host && _roomOpen) || (_mode == Mode.Client && _clientState == ClientState.Connected);

    /// O meu id na sala (o mesmo que os outros usam para me identificar).
    private ulong MySfxUserId => _mode == Mode.Host ? HostUserId : _myId;

    /// Chamado uma vez no construtor (depois do InitVolume e antes do InitFullscreen:
    /// o Esc fecha o painel antes de sair da tela cheia).
    private void InitSoundboard()
    {
        SfxPlayer.SetVolume(_preferences.SfxVolume);

        BuildSfxButton();
        BuildSfxPanel();
        BuildSfxVolumeBox();
        BuildSfxAddDialog();

        // Por cima do vídeo (mesmo Grid da imagem e do botão de volume)
        if (_previewImage.Parent is Panel stage)
        {
            stage.Children.Add(_sfxButton);
            stage.Children.Add(_sfxPanel);
            stage.Children.Add(_sfxVolumeBox);
        }

        // A janela de adicionar fica por cima de tudo (como a do avatar)
        if (Content is Panel root) root.Children.Add(_sfxAddOverlay);

        // Clicar fora do painel, do slider e do botão fecha o painel
        AddHandler(PointerPressedEvent, (_, e) =>
        {
            if (!_sfxPanelOpen || _sfxAddOpen || e.Source is not Visual source) return;

            if (_sfxVolumeBox.IsVisible && !IsInside(source, _sfxVolumeBox) && !IsInside(source, _sfxVolumeButton))
                HideSfxVolume();

            if (IsInside(source, _sfxPanel) || IsInside(source, _sfxButton) || IsInside(source, _sfxVolumeBox)) return;
            SetSoundboardPanel(false);
        }, RoutingStrategies.Tunnel);

        // Esc: fecha a janela de adicionar, depois cancela a renomeação, depois o slider, depois o painel
        AddHandler(KeyDownEvent, (_, e) =>
        {
            if (e.Key != Key.Escape) return;

            if (_sfxAddOpen) CloseSfxAddDialog();
            else if (!_sfxPanelOpen) return;
            else if (_sfxRenamingId is not null) CancelSfxRename();
            else if (_sfxVolumeBox.IsVisible) HideSfxVolume();
            else SetSoundboardPanel(false);
            e.Handled = true;
        }, RoutingStrategies.Tunnel);

        _sfxVolumeHideTimer.Interval = SfxVolumeHideDelay;
        _sfxVolumeHideTimer.Tick += (_, _) =>
        {
            _sfxVolumeHideTimer.Stop();
            if (_sfxVolumePinned || _sfxVolumeHeld) return;
            if (_sfxVolumeButton.IsPointerOver || _sfxVolumeBox.IsPointerOver) return;
            _sfxVolumeBox.IsVisible = false;
        };

        Closing += (_, _) => SfxPlayer.StopAll();

        RefreshSfxVolume();
    }

    // ───────────────────────── Botão 🎉 ─────────────────────────

    private void BuildSfxButton()
    {
        _sfxButton = new Border
        {
            Width = SfxButtonSize,
            Height = SfxButtonSize,
            CornerRadius = new CornerRadius(SfxButtonSize / 2),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Bottom,
            Margin = new Thickness(0, 0, 0, VolumeEdgeMargin),
            Cursor = new Cursor(StandardCursorType.Hand),
            Opacity = SfxIdleOpacity,
            RenderTransform = SfxScaleIdle,
            ZIndex = 20,
            IsVisible = false,
            Transitions = new Transitions
            {
                new DoubleTransition { Property = OpacityProperty, Duration = SfxHoverAnimation },
                new TransformOperationsTransition
                {
                    Property = RenderTransformProperty,
                    Duration = SfxHoverAnimation,
                    Easing = new CubicEaseOut(),
                },
            },
            Child = new TextBlock
            {
                Text = PartyEmoji,
                FontFamily = SfxEmojiFont,
                FontSize = 20,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        ToolTip.SetTip(_sfxButton, Loc.T("Sfx_Title"));

        _sfxButton.PointerEntered += (_, _) => SetSfxButtonHot(true);
        _sfxButton.PointerExited += (_, _) => SetSfxButtonHot(_sfxPanelOpen);
        _sfxButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_sfxButton).Properties.IsLeftButtonPressed) return;
            SetSoundboardPanel(!_sfxPanelOpen);
            e.Handled = true;
        };
    }

    // ───────────────────────── Painel ─────────────────────────

    private void BuildSfxPanel()
    {
        // ── Título + ícone do volume dos efeitos ──
        _sfxVolumeIcon = VolumeGlyph(15);
        _sfxVolumeButton = new Border
        {
            Width = 30,
            Height = 30,
            CornerRadius = new CornerRadius(6),
            Background = Brushes.Transparent,
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = _sfxVolumeIcon,
        };
        ToolTip.SetTip(_sfxVolumeButton, Loc.T("Sfx_VolumeTooltip"));
        Grid.SetColumn(_sfxVolumeButton, 1);

        var header = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("*,Auto"),
            Margin = new Thickness(2, 0, 0, 10),
            Children =
            {
                new TextBlock
                {
                    Text = Loc.T("Sfx_Title"),
                    FontSize = 17,
                    FontWeight = FontWeight.SemiBold,
                    VerticalAlignment = VerticalAlignment.Center,
                },
                _sfxVolumeButton,
            },
        };

        // ── Renomear (só aparece enquanto renomeia) ──
        _sfxRenameBox = new TextBox
        {
            PlaceholderText = Loc.T("Sfx_RenamePlaceholder"),
            MaxLength = 64,
            FontFamily = SfxTextFont,
        };
        var renameSave = new Button { Content = Loc.T("Common_Save"), Margin = new Thickness(6, 0, 0, 0) };
        renameSave.Classes.Add("accent");
        var renameCancel = new Button { Content = Loc.T("Common_Cancel"), Margin = new Thickness(6, 0, 0, 0) };
        Grid.SetColumn(renameSave, 1);
        Grid.SetColumn(renameCancel, 2);

        _sfxRenameRow = new Grid
        {
            ColumnDefinitions = new ColumnDefinitions("*,Auto,Auto"),
            Margin = new Thickness(0, 0, 0, 10),
            IsVisible = false,
            Children = { _sfxRenameBox, renameSave, renameCancel },
        };

        // ── Erro e lista vazia ──
        _sfxError = new TextBlock
        {
            Foreground = SfxErrorBrush,
            FontSize = 12,
            TextWrapping = TextWrapping.Wrap,
            Margin = new Thickness(2, 0, 2, 8),
            IsVisible = false,
        };
        _sfxEmptyText = new TextBlock
        {
            Text = Loc.T("Sfx_Empty"),
            FontSize = 12,
            Opacity = 0.65,
            TextWrapping = TextWrapping.Wrap,
            TextAlignment = TextAlignment.Center,
            Margin = new Thickness(8, 12),
            IsVisible = false,
        };

        // ── Os sons: 3 por fileira, rolando para baixo ──
        _sfxGrid = new UniformGrid { Columns = 3 };
        _sfxScroll = new ScrollViewer
        {
            MaxHeight = SfxListMaxHeight,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
            Content = _sfxGrid,
        };

        // ── Adicionar som (borda tracejada) ──
        _sfxAddButton = new Border
        {
            Height = 36,
            MinWidth = 150,
            CornerRadius = new CornerRadius(8),
            Background = Brushes.Transparent,
            HorizontalAlignment = HorizontalAlignment.Right,
            Margin = new Thickness(0, 10, 0, 0),
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = new Panel
            {
                Children =
                {
                    DashedOutline(8),
                    new StackPanel
                    {
                        Orientation = Orientation.Horizontal,
                        Spacing = 8,
                        Margin = new Thickness(14, 0),
                        HorizontalAlignment = HorizontalAlignment.Center,
                        VerticalAlignment = VerticalAlignment.Center,
                        Children =
                        {
                            new TextBlock
                            {
                                Text = "", // +
                                FontFamily = IconFont,
                                FontSize = 12,
                                VerticalAlignment = VerticalAlignment.Center,
                            },
                            new TextBlock { Text = Loc.T("Sfx_Add"), VerticalAlignment = VerticalAlignment.Center },
                        },
                    },
                },
            },
        };

        _sfxPanel = new Border
        {
            Width = SfxPanelWidth,
            CornerRadius = new CornerRadius(12),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(14),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Bottom,
            // Logo acima do botão 🎉
            Margin = new Thickness(0, 0, 0, SfxPanelBottom),
            BoxShadow = BoxShadows.Parse("0 8 24 0 #80000000"),
            ZIndex = 21,
            IsVisible = false,
            Child = new StackPanel
            {
                Children = { header, _sfxRenameRow, _sfxError, _sfxEmptyText, _sfxScroll, _sfxAddButton },
            },
        };

        // O slider de volume fica logo acima do painel: acompanha a altura dele
        _sfxPanel.SizeChanged += (_, e) => PositionSfxVolumeBox(e.NewSize.Height);

        // ── Eventos ──

        // Volume: passar o mouse mostra; clicar fixa (ou solta)
        _sfxVolumeButton.PointerEntered += (_, _) =>
        {
            _sfxVolumeButton.Background = VolumeBackgroundHover;
            ShowSfxVolume();
        };
        _sfxVolumeButton.PointerExited += (_, _) =>
        {
            _sfxVolumeButton.Background = Brushes.Transparent;
            ScheduleSfxVolumeHide();
        };
        _sfxVolumeButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_sfxVolumeButton).Properties.IsLeftButtonPressed) return;
            if (_sfxVolumePinned)
            {
                HideSfxVolume();
            }
            else
            {
                _sfxVolumePinned = true;
                ShowSfxVolume();
            }
            e.Handled = true;
        };
        _sfxVolumeButton.PointerWheelChanged += OnSfxVolumeWheel;

        // Adicionar som: abre a janela
        _sfxAddButton.PointerEntered += (_, _) => _sfxAddButton.Background = VolumeBackgroundHover;
        _sfxAddButton.PointerExited += (_, _) => _sfxAddButton.Background = Brushes.Transparent;
        _sfxAddButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_sfxAddButton).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            OpenSfxAddDialog();
        };

        // Soltar um arquivo no painel: abre a janela já com ele
        DragDrop.SetAllowDrop(_sfxPanel, true);
        _sfxPanel.AddHandler(DragDrop.DragOverEvent, OnSfxDragOver);
        _sfxPanel.AddHandler(DragDrop.DropEvent, async (_, e) =>
        {
            e.Handled = true;
            if (DroppedFile(e) is not { } file) return;
            OpenSfxAddDialog();
            await LoadSfxAddFileAsync(file);
        });

        // Renomear
        renameSave.Click += (_, _) => CommitSfxRename();
        renameCancel.Click += (_, _) => CancelSfxRename();
        _sfxRenameBox.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            CommitSfxRename();
            e.Handled = true;
        };
    }

    /// Distância do painel até a borda de baixo do vídeo (fica logo acima do botão 🎉).
    private static double SfxPanelBottom => VolumeEdgeMargin + SfxButtonSize + 10;

    /// Contorno tracejado com cantos arredondados (botão "Adicionar som" e área de soltar).
    private static Rectangle DashedOutline(double radius, double thickness = 1.5, IBrush? stroke = null) => new()
    {
        Stroke = stroke ?? SfxDashBrush,
        StrokeThickness = thickness,
        StrokeDashArray = new AvaloniaList<double> { 4, 3 },
        RadiusX = radius,
        RadiusY = radius,
    };

    /// Um som do painel: clicar toca; botão direito mostra as opções.
    private Control BuildSfxTile(SfxEntry entry)
    {
        var tile = new Button
        {
            Content = new TextBlock
            {
                Text = entry.Name,
                FontFamily = SfxTextFont,
                TextTrimming = TextTrimming.CharacterEllipsis,
                TextAlignment = TextAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
            Height = SfxTileHeight,
            Margin = new Thickness(3),
            Padding = new Thickness(8, 0),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Center,
            VerticalContentAlignment = VerticalAlignment.Center,
        };
        ToolTip.SetTip(tile, entry.Name);
        tile.Click += (_, _) => PlaySfx(entry);

        var preview = new MenuItem { Header = Loc.T("Sfx_PlayLocal") };
        preview.Click += (_, _) => PlaySfx(entry, onlyForMe: true);
        var rename = new MenuItem { Header = Loc.T("Sfx_Rename") };
        rename.Click += (_, _) => StartSfxRename(entry);
        var remove = new MenuItem { Header = Loc.T("Sfx_Remove") };
        remove.Click += (_, _) => RemoveSfx(entry);

        tile.ContextMenu = new ContextMenu { ItemsSource = new[] { preview, rename, remove } };
        return tile;
    }

    /// Mostra ou esconde o botão 🎉 (chamado pelo UpdateUi do Host e pelo UpdateVolumeControls).
    private void UpdateSoundboardControls()
    {
        if (!SoundboardAvailable)
        {
            CloseSfxAddDialog();
            SetSoundboardPanel(false);
            _sfxButton.IsVisible = false;
            return;
        }

        // Em tela cheia ele some junto com o botão de volume quando o mouse fica parado
        _sfxButton.IsVisible = !(_fullscreen && _volumeHiddenByIdle);
    }

    private void SetSoundboardPanel(bool open)
    {
        if (open && !SoundboardAvailable) open = false;
        if (open == _sfxPanelOpen) return;

        _sfxPanelOpen = open;
        _sfxPanel.IsVisible = open;

        if (open)
        {
            SetVolumePanel(false); // um painel por vez
            ReloadSfx();
            RefreshSfxVolume();
        }
        else
        {
            CancelSfxRename();
            HideSfxVolume();
            ShowSfxError(null);

            // Em tela cheia, volta a contar o tempo para esconder os botões
            if (_fullscreen && VolumeAvailable)
            {
                _volumeHideTimer.Stop();
                _volumeHideTimer.Start();
            }
        }

        SetSfxButtonHot(open || _sfxButton.IsPointerOver);
    }

    /// Botão "aceso" (mouse em cima ou painel aberto): visível por inteiro e um pouco maior.
    private void SetSfxButtonHot(bool hot)
    {
        _sfxButton.Opacity = hot ? 1 : SfxIdleOpacity;
        _sfxButton.RenderTransform = hot ? SfxScaleHot : SfxScaleIdle;
    }

    /// Relê a biblioteca e refaz a grade.
    private void ReloadSfx()
    {
        _sfxEntries = SfxStore.All();

        _sfxGrid.Children.Clear();
        foreach (var entry in _sfxEntries) _sfxGrid.Children.Add(BuildSfxTile(entry));

        _sfxEmptyText.IsVisible = _sfxEntries.Count == 0;
        _sfxScroll.IsVisible = _sfxEntries.Count > 0;
    }

    private void ShowSfxError(string? message)
    {
        _sfxErrorTimer?.Dispose();
        _sfxErrorTimer = null;

        _sfxError.Text = message ?? "";
        _sfxError.IsVisible = !string.IsNullOrEmpty(message);

        if (_sfxError.IsVisible)
            _sfxErrorTimer = DispatcherTimer.RunOnce(() => ShowSfxError(null), SfxErrorDuration);
    }

    // ───────────────────────── Tocar ─────────────────────────

    /// Toca o som. `onlyForMe` = "Ouvir só para mim" (sem destaque na lista; na etapa de rede,
    /// também não vai para a sala).
    private async void PlaySfx(SfxEntry entry, bool onlyForMe = false)
    {
        try
        {
            var duration = await SfxPlayer.PlayFileAsync(SfxStore.PathOf(entry), _preferences.AudioOutputDevice);
            if (!onlyForMe) HighlightSfxUser(MySfxUserId, duration);
        }
        catch (SfxException ex)
        {
            ShowSfxError(ex.Message);
        }
        catch (Exception ex)
        {
            ShowSfxError(Loc.F("Sfx_PlayError", ex.Message));
        }
    }

    // ───────────────────────── Destaque na lista de usuários ─────────────────────────

    /// Borda azul + 🎉 na linha de quem tocou um som, enquanto o som toca.
    /// (Na etapa de rede, também é chamado quando chega o som de outra pessoa.)
    private void HighlightSfxUser(ulong userId, TimeSpan soundDuration)
    {
        var span = soundDuration + SfxHighlightExtra;
        if (span < SfxHighlightMin) span = SfxHighlightMin;

        _sfxHighlights[userId] = DateTime.UtcNow + span;
        RefreshSfxUserLists();

        // Um pouco depois do fim: tira o destaque (se não tiver tocado outro som nesse meio tempo)
        DispatcherTimer.RunOnce(ExpireSfxHighlights, span + TimeSpan.FromMilliseconds(50));
    }

    private void ExpireSfxHighlights()
    {
        var now = DateTime.UtcNow;
        var expired = _sfxHighlights.Where(h => h.Value <= now).Select(h => h.Key).ToList();
        if (expired.Count == 0) return;

        foreach (var id in expired) _sfxHighlights.Remove(id);
        RefreshSfxUserLists();
    }

    private void RefreshSfxUserLists()
    {
        if (_roomOpen) RefreshUsers();
        if (_clientState == ClientState.Connected) RefreshClientUsers();
    }

    /// Envolve uma linha da lista de usuários: borda azul e 🎉 quando a pessoa tocou um som.
    /// (A borda existe sempre, transparente, para as linhas não "pularem" ao acender.)
    private Control WithSfxHighlight(Control row, ulong userId)
    {
        var lit = _sfxHighlights.TryGetValue(userId, out var until) && until > DateTime.UtcNow;

        var content = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto") };
        content.Children.Add(row);

        if (lit)
        {
            var party = new TextBlock
            {
                Text = PartyEmoji,
                FontFamily = SfxEmojiFont,
                FontSize = 14,
                VerticalAlignment = VerticalAlignment.Center,
                Margin = new Thickness(6, 0, 0, 0),
            };
            Grid.SetColumn(party, 1);
            content.Children.Add(party);
        }

        return new Border
        {
            CornerRadius = new CornerRadius(8),
            BorderThickness = new Thickness(1.5),
            BorderBrush = lit ? SfxHighlightBorder : Brushes.Transparent,
            Background = lit ? SfxHighlightFill : Brushes.Transparent,
            Padding = new Thickness(5, 3),
            Child = content,
        };
    }

    // ───────────────────────── Renomear e remover ─────────────────────────

    private void StartSfxRename(SfxEntry entry)
    {
        _sfxRenamingId = entry.Id;
        _sfxRenameBox.Text = entry.Name;
        _sfxRenameRow.IsVisible = true;

        Dispatcher.UIThread.Post(() =>
        {
            _sfxRenameBox.Focus();
            _sfxRenameBox.SelectAll();
        });
    }

    private void CommitSfxRename()
    {
        if (_sfxRenamingId is not { } id) return;

        try
        {
            SfxStore.Rename(id, _sfxRenameBox.Text); // nome vazio: mantém o anterior
        }
        catch (Exception ex)
        {
            ShowSfxError(Loc.F("Sfx_SaveError", ex.Message));
        }

        CancelSfxRename();
        ReloadSfx();
    }

    private void CancelSfxRename()
    {
        _sfxRenamingId = null;
        _sfxRenameRow.IsVisible = false;
        _sfxRenameBox.Text = "";
    }

    private void RemoveSfx(SfxEntry entry)
    {
        if (_sfxRenamingId == entry.Id) CancelSfxRename();

        try
        {
            SfxStore.Remove(entry.Id);
        }
        catch (Exception ex)
        {
            ShowSfxError(Loc.F("Sfx_RemoveError", ex.Message));
        }

        ReloadSfx();
    }

    // ───────────────────────── Volume dos efeitos (slider deitado) ─────────────────────────

    /// Cartão com o slider horizontal. Fica fora do painel, logo acima dele, alinhado à direita
    /// (embaixo dele está o ícone de volume).
    private void BuildSfxVolumeBox()
    {
        _sfxVolumeLabel = new TextBlock
        {
            FontSize = 12,
            MinWidth = 36,
            TextAlignment = TextAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(10, 0, 0, 0),
        };
        Grid.SetColumn(_sfxVolumeLabel, 1);

        _sfxVolumeSlider = new Slider
        {
            Orientation = Orientation.Horizontal,
            Minimum = 0,
            Maximum = 100,
            SmallChange = 1,
            LargeChange = 10,
            VerticalAlignment = VerticalAlignment.Center,
        };

        _sfxVolumeBox = new Border
        {
            Width = SfxVolumeBoxWidth,
            CornerRadius = new CornerRadius(10),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(14, 10, 14, 4),
            // Centralizado com um recuo à esquerda: a borda direita fica alinhada com a do painel
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Bottom,
            BoxShadow = BoxShadows.Parse("0 6 18 0 #80000000"),
            ZIndex = 22,
            IsVisible = false,
            Child = new StackPanel
            {
                Children =
                {
                    new TextBlock
                    {
                        Text = Loc.T("Sfx_VolumeTooltip"),
                        FontSize = 12,
                        Opacity = 0.85,
                    },
                    new Grid
                    {
                        ColumnDefinitions = new ColumnDefinitions("*,Auto"),
                        Children = { _sfxVolumeSlider, _sfxVolumeLabel },
                    },
                },
            },
        };
        PositionSfxVolumeBox(0);

        _sfxVolumeBox.PointerEntered += (_, _) => _sfxVolumeHideTimer.Stop();
        _sfxVolumeBox.PointerExited += (_, _) => ScheduleSfxVolumeHide();
        _sfxVolumeBox.PointerWheelChanged += OnSfxVolumeWheel;

        // Arrastando o slider, ele não some mesmo se o mouse sair do cartão
        _sfxVolumeBox.AddHandler(PointerPressedEvent, (_, _) => _sfxVolumeHeld = true,
            RoutingStrategies.Tunnel, handledEventsToo: true);
        _sfxVolumeBox.AddHandler(PointerReleasedEvent, (_, _) =>
        {
            _sfxVolumeHeld = false;
            ScheduleSfxVolumeHide();
        }, RoutingStrategies.Tunnel, handledEventsToo: true);

        _sfxVolumeSlider.ValueChanged += (_, _) =>
        {
            if (_updatingSfxSlider) return;
            SetSfxVolume((int)Math.Round(_sfxVolumeSlider.Value));
        };
    }

    /// Coloca o slider logo acima do painel (que muda de altura conforme a quantidade de sons).
    private void PositionSfxVolumeBox(double panelHeight)
    {
        _sfxVolumeBox.Margin = new Thickness(
            SfxPanelWidth - SfxVolumeBoxWidth,
            0,
            0,
            SfxPanelBottom + panelHeight + SfxVolumeGap);
    }

    private void ShowSfxVolume()
    {
        _sfxVolumeHideTimer.Stop();
        PositionSfxVolumeBox(_sfxPanel.Bounds.Height);
        _sfxVolumeBox.IsVisible = true;
    }

    private void HideSfxVolume()
    {
        _sfxVolumeHideTimer.Stop();
        _sfxVolumePinned = false;
        _sfxVolumeHeld = false;
        _sfxVolumeBox.IsVisible = false;
    }

    private void ScheduleSfxVolumeHide()
    {
        if (_sfxVolumePinned || _sfxVolumeHeld) return;
        _sfxVolumeHideTimer.Stop();
        _sfxVolumeHideTimer.Start();
    }

    /// Muda o volume dos efeitos: vale na hora (até para sons tocando) e é salvo logo depois.
    private void SetSfxVolume(int percent)
    {
        percent = Math.Clamp(percent, 0, 100);
        if (percent == _preferences.SfxVolume) return;

        _preferences.SfxVolume = percent;
        SfxPlayer.SetVolume(percent);
        RefreshSfxVolume();

        // Mesmo salvamento atrasado do volume da transmissão
        _volumeSaveTimer.Stop();
        _volumeSaveTimer.Start();
    }

    private void OnSfxVolumeWheel(object? sender, PointerWheelEventArgs e)
    {
        var step = e.Delta.Y > 0 ? SfxWheelStep : e.Delta.Y < 0 ? -SfxWheelStep : 0;
        if (step == 0) return;

        SetSfxVolume(_preferences.SfxVolume + step);
        e.Handled = true;
    }

    /// Ícone, texto e slider de acordo com o volume atual.
    private void RefreshSfxVolume()
    {
        var volume = _preferences.SfxVolume;

        _sfxVolumeIcon.Text = volume == 0 ? Volume0Glyph
            : volume < 34 ? Volume1Glyph
            : volume < 67 ? Volume2Glyph
            : Volume3Glyph;
        _sfxVolumeLabel.Text = $"{volume}%";

        _updatingSfxSlider = true;
        _sfxVolumeSlider.Value = volume;
        _updatingSfxSlider = false;
    }

    // ───────────────────────── Janela "Adicionar som" ─────────────────────────

    /// Igual à do avatar: área para clicar ou arrastar o arquivo; depois, o arquivo escolhido,
    /// o botão de ouvir e o campo do nome (o nome do arquivo já vem preenchido).
    private void BuildSfxAddDialog()
    {
        // ── Área de escolher/soltar ──
        _sfxAddDropTitle = new TextBlock
        {
            Text = Loc.T("Sfx_ClickToChoose"),
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        _sfxAddDrop = new Border
        {
            Height = 170,
            Background = SfxDropBackground,
            BorderBrush = SfxDropBorder,
            BorderThickness = new Thickness(2),
            CornerRadius = new CornerRadius(10),
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = new StackPanel
            {
                Spacing = 8,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
                Children =
                {
                    new TextBlock
                    {
                        Text = SfxAudioGlyph,
                        FontFamily = IconFont,
                        FontSize = 36,
                        Opacity = 0.7,
                        HorizontalAlignment = HorizontalAlignment.Center,
                    },
                    _sfxAddDropTitle,
                    new TextBlock
                    {
                        Text = Loc.T("Sfx_OrDrag"),
                        Opacity = 0.6,
                        HorizontalAlignment = HorizontalAlignment.Center,
                    },
                    new TextBlock
                    {
                        Text = Loc.T("Sfx_Formats"),
                        FontSize = 11,
                        Opacity = 0.45,
                        HorizontalAlignment = HorizontalAlignment.Center,
                    },
                },
            },
        };

        // ── Arquivo escolhido: ícone, nome, duração e "Ouvir" ──
        _sfxAddFileName = new TextBlock
        {
            FontWeight = FontWeight.SemiBold,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        _sfxAddFileInfo = new TextBlock { FontSize = 12, Opacity = 0.65 };

        _sfxAddPreview = new Button
        {
            Padding = new Thickness(10, 6),
            VerticalAlignment = VerticalAlignment.Center,
            Content = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = 6,
                Children =
                {
                    new TextBlock
                    {
                        Text = SfxPlayGlyph,
                        FontFamily = IconFont,
                        FontSize = 12,
                        VerticalAlignment = VerticalAlignment.Center,
                    },
                    new TextBlock { Text = Loc.T("Sfx_Preview"), VerticalAlignment = VerticalAlignment.Center },
                },
            },
        };

        var fileIcon = new TextBlock
        {
            Text = SfxAudioGlyph,
            FontFamily = IconFont,
            FontSize = 22,
            Opacity = 0.8,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 12, 0),
        };
        var fileText = new StackPanel
        {
            Spacing = 2,
            VerticalAlignment = VerticalAlignment.Center,
            Children = { _sfxAddFileName, _sfxAddFileInfo },
        };
        Grid.SetColumn(fileText, 1);
        Grid.SetColumn(_sfxAddPreview, 2);

        _sfxAddFileCard = new Border
        {
            Background = SfxDropBackground,
            BorderBrush = SfxDropBorder,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(10),
            Padding = new Thickness(14, 12),
            IsVisible = false,
            Child = new Grid
            {
                ColumnDefinitions = new ColumnDefinitions("Auto,*,Auto"),
                Children = { fileIcon, fileText, _sfxAddPreview },
            },
        };

        _sfxAddChooseAnother = new Button
        {
            Content = Loc.T("Sfx_ChooseAnother"),
            HorizontalAlignment = HorizontalAlignment.Center,
            IsVisible = false,
        };

        // ── Nome ──
        _sfxAddNameBox = new TextBox
        {
            PlaceholderText = Loc.T("Sfx_RenamePlaceholder"),
            MaxLength = 64,
            FontFamily = SfxTextFont,
        };
        _sfxAddNameSection = new StackPanel
        {
            Spacing = 6,
            IsVisible = false,
            Children =
            {
                new TextBlock { Text = Loc.T("Sfx_NameLabel"), FontSize = 12, Opacity = 0.8 },
                _sfxAddNameBox,
                new TextBlock
                {
                    Text = Loc.T("Sfx_NameHint"),
                    FontSize = 11,
                    Opacity = 0.5,
                    TextWrapping = TextWrapping.Wrap,
                },
            },
        };

        _sfxAddError = new TextBlock
        {
            Foreground = SfxErrorBrush,
            FontSize = 12,
            TextWrapping = TextWrapping.Wrap,
            TextAlignment = TextAlignment.Center,
            IsVisible = false,
        };

        // ── Cancelar / Salvar ──
        var cancel = new Button
        {
            Content = Loc.T("Common_Cancel"),
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Center,
            Padding = new Thickness(0, 10),
        };
        _sfxAddSave = new Button
        {
            Content = Loc.T("Common_Save"),
            IsEnabled = false,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Center,
            Padding = new Thickness(0, 10),
        };
        _sfxAddSave.Classes.Add("accent");
        Grid.SetColumn(_sfxAddSave, 2);

        var card = new Border
        {
            Background = SfxDialogBackground,
            BorderBrush = SfxDialogBorder,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(12),
            Padding = new Thickness(24),
            Width = 380,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
            BoxShadow = BoxShadows.Parse("0 12 40 0 #80000000"),
            Child = new StackPanel
            {
                Spacing = 16,
                Children =
                {
                    new TextBlock
                    {
                        Text = Loc.T("Sfx_AddTitle"),
                        FontSize = 20,
                        FontWeight = FontWeight.SemiBold,
                        HorizontalAlignment = HorizontalAlignment.Center,
                    },
                    _sfxAddDrop,
                    _sfxAddFileCard,
                    _sfxAddChooseAnother,
                    _sfxAddNameSection,
                    _sfxAddError,
                    new Grid
                    {
                        ColumnDefinitions = new ColumnDefinitions("*,8,*"),
                        Children = { cancel, _sfxAddSave },
                    },
                },
            },
        };

        _sfxAddOverlay = new Border
        {
            Background = new SolidColorBrush(Color.Parse("#B3000000")),
            IsVisible = false,
            ZIndex = 40,
            Child = card,
        };

        // ── Eventos ──
        _sfxAddDrop.PointerEntered += (_, _) => _sfxAddDrop.BorderBrush = SfxDropBorderHot;
        _sfxAddDrop.PointerExited += (_, _) => _sfxAddDrop.BorderBrush = SfxDropBorder;
        _sfxAddDrop.PointerPressed += async (_, e) =>
        {
            if (!e.GetCurrentPoint(_sfxAddDrop).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            await PickSfxAddFileAsync();
        };
        _sfxAddChooseAnother.Click += async (_, _) => await PickSfxAddFileAsync();
        _sfxAddPreview.Click += (_, _) => PreviewSfxAddFile();
        cancel.Click += (_, _) => CloseSfxAddDialog();
        _sfxAddSave.Click += async (_, _) => await SaveSfxAddAsync();
        _sfxAddNameBox.KeyDown += async (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            e.Handled = true;
            await SaveSfxAddAsync();
        };

        // Soltar o arquivo em qualquer lugar da janela
        DragDrop.SetAllowDrop(_sfxAddOverlay, true);
        _sfxAddOverlay.AddHandler(DragDrop.DragOverEvent, OnSfxDragOver);
        _sfxAddOverlay.AddHandler(DragDrop.DropEvent, async (_, e) =>
        {
            e.Handled = true;
            if (DroppedFile(e) is { } file) await LoadSfxAddFileAsync(file);
        });

        // Clicar na parte escurecida (fora da janela) cancela
        _sfxAddOverlay.PointerPressed += (_, e) =>
        {
            if (ReferenceEquals(e.Source, _sfxAddOverlay)) CloseSfxAddDialog();
        };
    }

    private void OnSfxDragOver(object? sender, DragEventArgs e)
    {
        e.DragEffects = e.DataTransfer.Contains(DataFormat.File) ? DragDropEffects.Copy : DragDropEffects.None;
        e.Handled = true;
    }

    private static IStorageFile? DroppedFile(DragEventArgs e) =>
        e.DataTransfer.TryGetFiles()?.OfType<IStorageFile>().FirstOrDefault();

    private void OpenSfxAddDialog()
    {
        if (_sfxAddOpen || !SoundboardAvailable) return;
        if (_pickerOpen || _avatarEditorOpen || _settingsOpen) return;

        _sfxAddOpen = true;
        ResetSfxAddDialog();
        HideSfxVolume();
        CancelSfxRename();

        _sfxAddOverlay.IsVisible = true;
        _mainLayout.IsEnabled = false; // o fundo fica travado, como na janela do avatar
    }

    private void CloseSfxAddDialog()
    {
        if (!_sfxAddOpen) return;

        _sfxAddOpen = false;
        _sfxAddOverlay.IsVisible = false;
        _mainLayout.IsEnabled = true;
        ResetSfxAddDialog();
    }

    /// Volta a janela para "escolha ou arraste um arquivo".
    private void ResetSfxAddDialog()
    {
        _sfxAddData = null;
        _sfxAddCheck = null;
        _sfxAddBusy = false;

        _sfxAddDrop.IsVisible = true;
        _sfxAddDropTitle.Text = Loc.T("Sfx_ClickToChoose");
        _sfxAddFileCard.IsVisible = false;
        _sfxAddChooseAnother.IsVisible = false;
        _sfxAddNameSection.IsVisible = false;
        _sfxAddNameBox.Text = "";
        _sfxAddSave.IsEnabled = false;
        SetSfxAddError(null);
    }

    private void SetSfxAddError(string? message)
    {
        _sfxAddError.Text = message ?? "";
        _sfxAddError.IsVisible = !string.IsNullOrEmpty(message);
    }

    private async Task PickSfxAddFileAsync()
    {
        if (_sfxAddBusy) return;

        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = Loc.T("Sfx_PickerTitle"),
            AllowMultiple = false,
            FileTypeFilter = new[] { SfxFileTypes },
        });

        if (files.Count > 0) await LoadSfxAddFileAsync(files[0]);
    }

    /// Lê e confere o arquivo (formato, tamanho, duração). Deu certo: mostra o arquivo e pede o nome.
    /// Deu errado: mostra o erro e mantém o arquivo anterior (se havia um).
    private async Task LoadSfxAddFileAsync(IStorageFile file)
    {
        if (_sfxAddBusy || !_sfxAddOpen) return;
        _sfxAddBusy = true;
        SetSfxAddError(null);

        var hadFile = _sfxAddCheck is not null;
        if (!hadFile) _sfxAddDropTitle.Text = Loc.T("Sfx_Checking");

        try
        {
            byte[] bytes;
            await using (var stream = await file.OpenReadAsync())
            {
                bytes = await ReadSfxFileAsync(stream);
            }

            var check = await Task.Run(() => SfxStore.Check(bytes));
            if (!_sfxAddOpen) return; // a janela foi fechada enquanto conferia

            _sfxAddData = bytes;
            _sfxAddCheck = check;

            _sfxAddFileName.Text = file.Name;
            ToolTip.SetTip(_sfxAddFileName, file.Name);
            _sfxAddFileInfo.Text = Loc.F("Sfx_Duration", check.Duration.TotalSeconds);

            _sfxAddDrop.IsVisible = false;
            _sfxAddFileCard.IsVisible = true;
            _sfxAddChooseAnother.IsVisible = true;
            _sfxAddNameSection.IsVisible = true;
            _sfxAddSave.IsEnabled = true;

            // Nome padrão: o nome do arquivo (dá para mudar e colar um emoji)
            _sfxAddNameBox.Text = SfxStore.DefaultName(file.Name);
            Dispatcher.UIThread.Post(() =>
            {
                _sfxAddNameBox.Focus();
                _sfxAddNameBox.SelectAll();
            });
        }
        catch (SfxException ex)
        {
            SetSfxAddError(ex.Message);
        }
        catch (Exception ex)
        {
            SetSfxAddError(Loc.F("Sfx_SaveError", ex.Message));
        }
        finally
        {
            _sfxAddBusy = false;
            if (_sfxAddCheck is null) _sfxAddDropTitle.Text = Loc.T("Sfx_ClickToChoose");
        }
    }

    /// "Ouvir": toca o arquivo escolhido só neste PC (sem destaque na lista).
    private async void PreviewSfxAddFile()
    {
        if (_sfxAddCheck is not { } check) return;

        try
        {
            await SfxPlayer.PlaySamplesAsync(check.Samples, _preferences.AudioOutputDevice);
        }
        catch (Exception ex)
        {
            SetSfxAddError(ex is SfxException ? ex.Message : Loc.F("Sfx_PlayError", ex.Message));
        }
    }

    /// Grava o som com o nome escolhido e volta para o painel (com o som novo no fim da lista).
    private async Task SaveSfxAddAsync()
    {
        if (_sfxAddBusy || _sfxAddData is not { } data || _sfxAddCheck is not { } check) return;
        _sfxAddBusy = true;
        _sfxAddSave.IsEnabled = false;

        var name = _sfxAddNameBox.Text;
        try
        {
            await Task.Run(() => SfxStore.Add(data, check.Format, name));
        }
        catch (Exception ex)
        {
            _sfxAddBusy = false;
            _sfxAddSave.IsEnabled = true;
            SetSfxAddError(Loc.F("Sfx_SaveError", ex.Message));
            return;
        }

        CloseSfxAddDialog();

        if (_sfxPanelOpen)
        {
            ReloadSfx();
            Dispatcher.UIThread.Post(() => _sfxScroll.ScrollToEnd(), DispatcherPriority.Background);
        }
    }

    /// Lê o arquivo para a memória, parando se passar do tamanho máximo.
    private static async Task<byte[]> ReadSfxFileAsync(Stream stream)
    {
        using var memory = new MemoryStream();
        var buffer = new byte[81920];
        int read;
        while ((read = await stream.ReadAsync(buffer)) > 0)
        {
            memory.Write(buffer, 0, read);
            if (memory.Length > SfxStore.MaxFileBytes) throw new SfxException(Loc.T("Sfx_TooBig"));
        }
        return memory.ToArray();
    }
}