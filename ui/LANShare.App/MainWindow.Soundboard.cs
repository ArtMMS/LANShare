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
/// botão 🎉 na parte de baixo do vídeo e, ao clicar, o painel com os sons (3 por fileira),
/// o "Adicionar som" e o volume dos efeitos no canto superior direito.
/// Por enquanto os sons tocam só neste PC (a rede vem na próxima etapa).
public partial class MainWindow
{
    private const double SfxButtonSize = 44;
    private const double SfxPanelWidth = 360;
    private const double SfxListMaxHeight = 196;
    private const double SfxTileHeight = 40;

    /// Opacidade do botão parado (ele fica "escondido" até passar o mouse).
    private const double SfxIdleOpacity = 0.35;

    /// Quanto a roda do mouse muda o volume dos efeitos a cada "clique".
    private const int SfxWheelStep = 5;

    private static readonly TimeSpan SfxHoverAnimation = TimeSpan.FromMilliseconds(160);
    private static readonly TimeSpan SfxErrorDuration = TimeSpan.FromSeconds(5);

    /// O slider do volume some este tempo depois de o mouse sair dele (se não estiver fixado).
    private static readonly TimeSpan SfxVolumeHideDelay = TimeSpan.FromMilliseconds(350);

    private static readonly TransformOperations SfxScaleIdle = TransformOperations.Parse("scale(1)");
    private static readonly TransformOperations SfxScaleHot = TransformOperations.Parse("scale(1.12)");

    /// Nome dos sons: texto normal, com fallback para emoji colorido.
    private static readonly FontFamily SfxTextFont = new("Segoe UI, Segoe UI Emoji");
    private static readonly FontFamily SfxEmojiFont = new("Segoe UI Emoji");

    private static readonly IBrush SfxDashBrush = new SolidColorBrush(Color.Parse("#5A6470"));
    private static readonly IBrush SfxErrorBrush = new SolidColorBrush(Color.Parse("#FF6B6B"));

    private static readonly FilePickerFileType SfxFileTypes = new(Loc.T("Sfx_FileType"))
    {
        Patterns = new[] { "*.mp3", "*.wav", "*.ogg" },
    };

    private Border _sfxButton = null!, _sfxPanel = null!, _sfxVolumeButton = null!, _sfxVolumeBox = null!,
        _sfxAddButton = null!;
    private TextBlock _sfxVolumeIcon = null!, _sfxVolumeLabel = null!, _sfxError = null!, _sfxEmptyText = null!,
        _sfxAddText = null!;
    private Slider _sfxVolumeSlider = null!;
    private UniformGrid _sfxGrid = null!;
    private ScrollViewer _sfxScroll = null!;
    private Grid _sfxRenameRow = null!;
    private TextBox _sfxRenameBox = null!;

    private readonly DispatcherTimer _sfxVolumeHideTimer = new();
    private IDisposable? _sfxErrorTimer;

    private List<SfxEntry> _sfxEntries = new();
    private bool _sfxPanelOpen;
    private bool _sfxVolumePinned;   // clicou no ícone: o slider fica aberto até clicar de novo
    private bool _sfxVolumeHeld;     // arrastando o slider (não esconde no meio)
    private bool _sfxImporting;
    private bool _updatingSfxSlider;
    private string? _sfxRenamingId;

    /// O soundboard faz sentido agora? (Host com a sala aberta ou Client conectado.)
    private bool SoundboardAvailable =>
        (_mode == Mode.Host && _roomOpen) || (_mode == Mode.Client && _clientState == ClientState.Connected);

    /// Chamado uma vez no construtor (depois do InitVolume e antes do InitFullscreen:
    /// o Esc fecha o painel antes de sair da tela cheia).
    private void InitSoundboard()
    {
        SfxPlayer.SetVolume(_preferences.SfxVolume);

        BuildSfxButton();
        BuildSfxPanel();

        // Por cima do vídeo (mesmo Grid da imagem e do botão de volume)
        if (_previewImage.Parent is Panel stage)
        {
            stage.Children.Add(_sfxButton);
            stage.Children.Add(_sfxPanel);
        }

        // Clicar fora do painel e do botão fecha o painel (e o slider, se clicar fora dele)
        AddHandler(PointerPressedEvent, (_, e) =>
        {
            if (!_sfxPanelOpen || e.Source is not Visual source) return;

            if (_sfxVolumeBox.IsVisible && !IsInside(source, _sfxVolumeBox) && !IsInside(source, _sfxVolumeButton))
                HideSfxVolume();

            if (IsInside(source, _sfxPanel) || IsInside(source, _sfxButton)) return;
            SetSoundboardPanel(false);
        }, RoutingStrategies.Tunnel);

        // Esc: cancela a renomeação, depois fecha o slider, depois fecha o painel
        AddHandler(KeyDownEvent, (_, e) =>
        {
            if (!_sfxPanelOpen || e.Key != Key.Escape) return;

            if (_sfxRenamingId is not null) CancelSfxRename();
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

    // ───────────────────────── Montagem ─────────────────────────

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
                Text = "\U0001F389", // 🎉
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
        _sfxAddText = new TextBlock { Text = Loc.T("Sfx_Add"), VerticalAlignment = VerticalAlignment.Center };
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
                    new Rectangle
                    {
                        Stroke = SfxDashBrush,
                        StrokeThickness = 1.5,
                        StrokeDashArray = new AvaloniaList<double> { 4, 3 },
                        RadiusX = 8,
                        RadiusY = 8,
                    },
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
                                Text = "\uE710", // +
                                FontFamily = IconFont,
                                FontSize = 12,
                                VerticalAlignment = VerticalAlignment.Center,
                            },
                            _sfxAddText,
                        },
                    },
                },
            },
        };

        // ── Slider do volume (abre por cima da lista, logo abaixo do ícone) ──
        _sfxVolumeLabel = new TextBlock
        {
            FontSize = 11,
            HorizontalAlignment = HorizontalAlignment.Center,
            Opacity = 0.9,
        };
        _sfxVolumeSlider = new Slider
        {
            Orientation = Orientation.Vertical,
            Minimum = 0,
            Maximum = 100,
            SmallChange = 1,
            LargeChange = 10,
            Height = 110,
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        _sfxVolumeBox = new Border
        {
            Width = 52,
            CornerRadius = new CornerRadius(8),
            Background = VolumeBackground,
            BorderBrush = VolumeBorder,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(4, 8),
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Top,
            Margin = new Thickness(0, 34, 0, 0),
            BoxShadow = BoxShadows.Parse("0 6 18 0 #80000000"),
            ZIndex = 5,
            IsVisible = false,
            Child = new StackPanel
            {
                Spacing = 6,
                Children = { _sfxVolumeLabel, _sfxVolumeSlider },
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
            Margin = new Thickness(0, 0, 0, VolumeEdgeMargin + SfxButtonSize + 10),
            BoxShadow = BoxShadows.Parse("0 8 24 0 #80000000"),
            ZIndex = 21,
            IsVisible = false,
            Child = new Panel
            {
                Children =
                {
                    new StackPanel
                    {
                        Children = { header, _sfxRenameRow, _sfxError, _sfxEmptyText, _sfxScroll, _sfxAddButton },
                    },
                    _sfxVolumeBox,
                },
            },
        };

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
        _sfxVolumeBox.PointerEntered += (_, _) => _sfxVolumeHideTimer.Stop();
        _sfxVolumeBox.PointerExited += (_, _) => ScheduleSfxVolumeHide();

        // Arrastando o slider, ele não some mesmo se o mouse sair do quadro
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
        _sfxVolumeButton.PointerWheelChanged += OnSfxVolumeWheel;
        _sfxVolumeBox.PointerWheelChanged += OnSfxVolumeWheel;

        // Adicionar som
        _sfxAddButton.PointerEntered += (_, _) => _sfxAddButton.Background = VolumeBackgroundHover;
        _sfxAddButton.PointerExited += (_, _) => _sfxAddButton.Background = Brushes.Transparent;
        _sfxAddButton.PointerPressed += async (_, e) =>
        {
            if (!e.GetCurrentPoint(_sfxAddButton).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            await PickSfxFilesAsync();
        };

        // Soltar arquivos em qualquer lugar do painel
        DragDrop.SetAllowDrop(_sfxPanel, true);
        _sfxPanel.AddHandler(DragDrop.DragOverEvent, (_, e) =>
        {
            e.DragEffects = e.DataTransfer.Contains(DataFormat.File) ? DragDropEffects.Copy : DragDropEffects.None;
            e.Handled = true;
        });
        _sfxPanel.AddHandler(DragDrop.DropEvent, async (_, e) =>
        {
            e.Handled = true;
            var files = e.DataTransfer.TryGetFiles()?.OfType<IStorageFile>().ToList();
            if (files is { Count: > 0 }) await ImportSfxFilesAsync(files);
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
        preview.Click += (_, _) => PlaySfx(entry);
        var rename = new MenuItem { Header = Loc.T("Sfx_Rename") };
        rename.Click += (_, _) => StartSfxRename(entry);
        var remove = new MenuItem { Header = Loc.T("Sfx_Remove") };
        remove.Click += (_, _) => RemoveSfx(entry);

        tile.ContextMenu = new ContextMenu { ItemsSource = new[] { preview, rename, remove } };
        return tile;
    }

    // ───────────────────────── Botão e painel ─────────────────────────

    /// Mostra ou esconde o botão 🎉 (chamado pelo UpdateUi do Host e pelo UpdateVolumeControls).
    private void UpdateSoundboardControls()
    {
        if (!SoundboardAvailable)
        {
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

    /// Toca o som neste PC. (Na etapa de rede, clicar também vai tocar para a sala.)
    private async void PlaySfx(SfxEntry entry)
    {
        try
        {
            await SfxPlayer.PlayFileAsync(SfxStore.PathOf(entry), _preferences.AudioOutputDevice);
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

    // ───────────────────────── Adicionar ─────────────────────────

    private async Task PickSfxFilesAsync()
    {
        if (_sfxImporting) return;

        var files = await StorageProvider.OpenFilePickerAsync(new FilePickerOpenOptions
        {
            Title = Loc.T("Sfx_PickerTitle"),
            AllowMultiple = true,
            FileTypeFilter = new[] { SfxFileTypes },
        });

        if (files.Count > 0) await ImportSfxFilesAsync(files);
    }

    /// Adiciona um ou mais arquivos, um por vez. Se algum falhar, os outros continuam.
    private async Task ImportSfxFilesAsync(IReadOnlyList<IStorageFile> files)
    {
        if (_sfxImporting) return;
        _sfxImporting = true;
        _sfxAddText.Text = Loc.T("Sfx_Adding");
        ShowSfxError(null);

        string? lastError = null;
        var added = 0;

        foreach (var file in files)
        {
            try
            {
                byte[] bytes;
                await using (var stream = await file.OpenReadAsync())
                {
                    bytes = await ReadSfxFileAsync(stream);
                }

                var name = file.Name;
                await Task.Run(() => SfxStore.Import(bytes, name));
                added++;
            }
            catch (SfxException ex)
            {
                lastError = files.Count > 1 ? $"{file.Name}: {ex.Message}" : ex.Message;
            }
            catch (Exception ex)
            {
                lastError = Loc.F("Sfx_SaveError", ex.Message);
            }
        }

        _sfxImporting = false;
        _sfxAddText.Text = Loc.T("Sfx_Add");

        if (added > 0)
        {
            ReloadSfx();
            Dispatcher.UIThread.Post(() => _sfxScroll.ScrollToEnd(), DispatcherPriority.Background);
        }
        ShowSfxError(lastError);
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

    // ───────────────────────── Volume dos efeitos ─────────────────────────

    private void ShowSfxVolume()
    {
        _sfxVolumeHideTimer.Stop();
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
}
