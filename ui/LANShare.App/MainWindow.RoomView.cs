using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Animation;
using Avalonia.Animation.Easings;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using System.Runtime.InteropServices;

namespace LANShare.App;

/// Tela da sala (v2.0), no Host e no Client: todos da sala em grade, como no Discord / Google Meet.
/// - a grade se ajusta ao número de pessoas (1, 2, 3–4, 5–6, 7–9...): colunas = ⌈√n⌉, a última
///   fileira fica centralizada;
/// - cada quadrado mostra o avatar (a câmera entra aqui depois), o nome e o ícone de tela de quem
///   transmite (clicar assiste, como na lista);
/// - o quadrado de quem toca um efeito sonoro acende (borda azul + 🎉), como na lista;
/// - transmitindo, a prévia da minha transmissão ganha um quadrado próprio, logo depois do meu
///   (o meu quadrado continua com o avatar; depois, com a câmera); clicar nele (ou no meu ícone
///   de tela na lista) mostra a prévia grande, como quem assiste vê; clicar nela volta à grade;
/// - barra de baixo (sobre a grade e sobre a transmissão assistida): 🎉 efeitos sonoros, câmera
///   (branco = desligada, verde = ligada; por enquanto desativada) e transmitir (ícone de monitor:
///   branco = sem transmissão, verde = transmitindo; abre o painel de escolha do que transmitir);
/// - é a tela padrão da sala: assistir alguém a substitui e parar de assistir volta para ela;
///   clicar em "USUÁRIOS NA SALA" ou num avatar da lista volta para ela (e para de assistir).
public partial class MainWindow
{
    private const double RoomTileGap = 12;
    private const double RoomTileAspect = 16.0 / 9.0;

    /// Com pouca gente, os quadrados não passam deste tamanho (1 pessoa não ocupa a tela toda).
    private const double RoomTileMaxWidth = 640;
    private const double RoomTileMinWidth = 120;

    private static readonly IBrush RoomTileBackground = new SolidColorBrush(Color.Parse("#1B1F24"));
    private static readonly IBrush RoomTileBorderIdle = new SolidColorBrush(Color.Parse("#262C34"));
    private static readonly IBrush RoomNameBackground = new SolidColorBrush(Color.Parse("#B3000000"));
    private static readonly IBrush RoomStreamBackground = new SolidColorBrush(Color.Parse("#0B0D10"));

    /// Uma pessoa da sala, do jeito que aparece na grade.
    private sealed record RoomMember(ulong Id, string Name, Bitmap? Avatar);

    private Panel _roomView = null!;
    private StackPanel _roomRows = null!;
    private List<RoomMember> _roomMembers = new();

    /// A imagem do quadrado da minha transmissão. É sempre a mesma (a grade é refeita várias
    /// vezes; ela só muda de quadrado), para a prévia não piscar.
    private readonly Image _ownStreamImage = new() { Stretch = Stretch.Uniform };
    private WriteableBitmap? _ownStreamBitmap;

    /// A prévia da minha transmissão está aberta grande (no lugar da grade)?
    private bool _ownPreviewBig;

    private const string CameraGlyph = "\uE714";
    private static readonly IBrush RoomBarIconOff = Brushes.White;

    /// Barra de baixo da sala: 🎉, câmera e transmitir.
    private StackPanel _roomBar = null!;
    private Border _cameraButton = null!, _shareButton = null!;
    private TextBlock _shareIcon = null!;

    /// Chamado uma vez no construtor (antes de qualquer UpdateStage).
    private void InitRoomView()
    {
        _roomView = Find<Panel>("RoomView");
        _roomRows = Find<StackPanel>("RoomRows");

        // O tamanho dos quadrados depende do espaço: refaz ao redimensionar (e ao aparecer)
        _roomView.SizeChanged += (_, _) => LayoutRoomTiles();

        // "USUÁRIOS NA SALA" (Host e Client) abre a tela da sala
        foreach (var header in new[] { Find<TextBlock>("HostUsersHeader"), Find<TextBlock>("ClientUsersHeader") })
        {
            header.Cursor = new Cursor(StandardCursorType.Hand);
            ToolTip.SetTip(header, Loc.T("Room_OpenTooltip"));
            header.PointerEntered += (_, _) => header.Opacity = 0.85;
            header.PointerExited += (_, _) => header.Opacity = 0.5;
            header.PointerPressed += (_, e) =>
            {
                if (!e.GetCurrentPoint(header).Properties.IsLeftButtonPressed) return;
                e.Handled = true;
                ShowRoomView();
            };
        }

        // Clicar na minha prévia grande volta para a grade (na transmissão assistida não faz nada)
        _previewImage.PointerPressed += (_, e) =>
        {
            if (!_ownPreviewBig || !e.GetCurrentPoint(_previewImage).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            CloseOwnPreviewBig();
        };
    }

    /// Volta para a tela da sala: fecha a minha prévia grande ou, se estiver assistindo alguém,
    /// para de assistir.
    private void ShowRoomView()
    {
        if (!InRoom) return;
        if (_ownPreviewBig)
        {
            CloseOwnPreviewBig();
            return;
        }
        if (_watchUser is null && _watchState == WatchState.None) return; // já está nela

        NativeMethods.WatchStop();
        EndWatch(null);
    }

    /// Estou numa sala? (Host com a sala aberta ou Client conectado.)
    private bool InRoom =>
        (_mode == Mode.Host && _roomOpen) || (_mode == Mode.Client && _clientState == ClientState.Connected);

    /// A grade aparece sempre que estou na sala, não estou assistindo ninguém e a minha
    /// prévia não está aberta grande.
    private bool RoomViewVisible => InRoom && _watchState == WatchState.None && !_ownPreviewBig;

    /// Troca quem aparece na grade (chamado junto com a lista da lateral).
    private void SetRoomMembers(List<RoomMember> members)
    {
        _roomMembers = members;
        LayoutRoomTiles();
    }

    /// Colunas e fileiras para `count` pessoas: 1 → 1x1, 2 → 2x1, 3–4 → 2x2, 5–6 → 3x2, 7–9 → 3x3...
    private static (int Columns, int Rows) RoomGridSize(int count)
    {
        if (count <= 0) return (0, 0);
        var columns = (int)Math.Ceiling(Math.Sqrt(count));
        var rows = (int)Math.Ceiling(count / (double)columns);
        return (columns, rows);
    }

    /// Monta a grade de novo, com o maior quadrado 16:9 que cabe no espaço.
    private void LayoutRoomTiles()
    {
        // A imagem da minha transmissão sai do quadrado antigo antes de entrar no novo
        if (_ownStreamImage.Parent is Panel oldParent) oldParent.Children.Remove(_ownStreamImage);
        _roomRows.Children.Clear();

        // Os quadrados na ordem da lista; transmitindo, o da minha transmissão vem logo depois do meu
        var tiles = new List<Func<double, double, Control>>();
        foreach (var member in _roomMembers)
        {
            tiles.Add((w, h) => BuildRoomTile(member, w, h));
            if (member.Id == MySfxUserId && _streaming)
                tiles.Add(BuildOwnStreamTile);
        }

        var count = tiles.Count;
        if (count == 0) return;

        // Ainda sem tamanho (escondida): o SizeChanged chama de novo quando ela aparecer
        var area = _roomView.Bounds.Size;
        if (area.Width <= 0 || area.Height <= 0) return;

        var (columns, rows) = RoomGridSize(count);
        var byWidth = (area.Width - RoomTileGap * (columns - 1)) / columns;
        var byHeight = (area.Height - RoomTileGap * (rows - 1)) / rows * RoomTileAspect;
        var width = Math.Clamp(Math.Min(byWidth, byHeight), RoomTileMinWidth, RoomTileMaxWidth);
        var height = width / RoomTileAspect;

        _roomRows.Spacing = RoomTileGap;
        for (var r = 0; r < rows; r++)
        {
            var row = new StackPanel
            {
                Orientation = Orientation.Horizontal,
                Spacing = RoomTileGap,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            foreach (var build in tiles.Skip(r * columns).Take(columns))
                row.Children.Add(build(width, height));
            _roomRows.Children.Add(row);
        }
    }

    /// Um quadrado da grade: avatar no meio, nome (+ ícone de tela) embaixo à esquerda
    /// e 🎉 em cima à direita enquanto a pessoa toca um efeito sonoro.
    private Control BuildRoomTile(RoomMember member, double width, double height)
    {
        var lit = _sfxHighlights.TryGetValue(member.Id, out var until) && until > DateTime.UtcNow;
        var content = new Grid();

        // Avatar (quando a câmera existir, a imagem dela entra no lugar)
        var avatarSize = Math.Clamp(height * 0.42, 40, 160);
        content.Children.Add(new Ellipse
        {
            Width = avatarSize,
            Height = avatarSize,
            Fill = AvatarBrush(member.Avatar),
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });

        // Nome + ícone de tela
        var label = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        label.Children.Add(new TextBlock
        {
            Text = member.Name,
            FontSize = 13,
            MaxWidth = Math.Max(40, width - 80),
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        if (IsStreamingUser(member.Id)) label.Children.Add(ScreenIcon(member.Id));

        content.Children.Add(new Border
        {
            Background = RoomNameBackground,
            CornerRadius = new CornerRadius(6),
            Padding = new Thickness(8, 3),
            Margin = new Thickness(10),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            Child = label,
        });

        if (lit)
        {
            content.Children.Add(new TextBlock
            {
                Text = PartyEmoji,
                FontFamily = SfxEmojiFont,
                FontSize = 18,
                Margin = new Thickness(10),
                HorizontalAlignment = HorizontalAlignment.Right,
                VerticalAlignment = VerticalAlignment.Top,
            });
        }

        return new Border
        {
            Width = width,
            Height = height,
            CornerRadius = new CornerRadius(12),
            Background = RoomTileBackground,
            BorderThickness = new Thickness(2),
            BorderBrush = lit ? SfxHighlightBorder : RoomTileBorderIdle,
            ClipToBounds = true,
            Child = content,
        };
    }

    // ───────────────────────── Quadrado da minha transmissão ─────────────────────────

    /// O quadrado com a prévia da minha transmissão (tela). "Transmitindo" aparece até a 1ª imagem.
    private Control BuildOwnStreamTile(double width, double height)
    {
        var content = new Grid();
        content.Children.Add(new TextBlock
        {
            Text = Loc.T("Host_Streaming"),
            Opacity = 0.6,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        });
        content.Children.Add(_ownStreamImage);

        var label = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        label.Children.Add(new TextBlock
        {
            Text = ScreenGlyph,
            FontFamily = IconFont,
            FontSize = 13,
            Foreground = WatchedBorder,
            VerticalAlignment = VerticalAlignment.Center,
        });
        label.Children.Add(new TextBlock
        {
            Text = Loc.T("Room_YourStream"),
            FontSize = 13,
            MaxWidth = Math.Max(40, width - 80),
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });

        content.Children.Add(new Border
        {
            Background = RoomNameBackground,
            CornerRadius = new CornerRadius(6),
            Padding = new Thickness(8, 3),
            Margin = new Thickness(10),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            Child = label,
        });

        var tile = new Border
        {
            Width = width,
            Height = height,
            CornerRadius = new CornerRadius(12),
            Background = RoomStreamBackground,
            BorderThickness = new Thickness(2),
            BorderBrush = RoomTileBorderIdle,
            ClipToBounds = true,
            Cursor = new Cursor(StandardCursorType.Hand),
            Child = content,
        };
        ToolTip.SetTip(tile, Loc.T("Room_YourStreamTooltip"));

        // Clicar abre a prévia grande
        tile.PointerEntered += (_, _) => tile.BorderBrush = WatchedBorder;
        tile.PointerExited += (_, _) => tile.BorderBrush = RoomTileBorderIdle;
        tile.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(tile).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            ShowOwnPreviewBig();
        };

        return tile;
    }

    /// Abre a prévia da minha transmissão grande, no lugar da grade (como quem assiste vê).
    /// Se eu estava assistindo alguém, para de assistir.
    private void ShowOwnPreviewBig()
    {
        if (!InRoom || !_streaming || _ownPreviewBig) return;

        if (_watchUser is not null || _watchState != WatchState.None)
        {
            NativeMethods.WatchStop();
            EndWatch(null);
        }

        _ownPreviewBig = true;
        HidePreview(Loc.T("Host_Streaming")); // até chegar a próxima imagem da prévia
    }

    /// Fecha a prévia grande e volta para a grade.
    private void CloseOwnPreviewBig()
    {
        if (!_ownPreviewBig) return;
        _ownPreviewBig = false;
        HidePreview(IdleStageText());
    }

    /// Pede a prévia mais recente da minha transmissão e desenha grande, na área do vídeo.
    private void UpdateOwnPreviewBig()
    {
        if (!NativeMethods.StreamPreview(_previewBuffer, out var width, out var height)) return;
        PaintFrame(width, height);
    }

    /// Pede a prévia mais recente da minha transmissão e desenha no quadrado dela
    /// (só quando a grade está na tela).
    private void UpdateOwnStreamTile()
    {
        if (!_roomView.IsVisible) return;
        if (!NativeMethods.StreamPreview(_previewBuffer, out var width, out var height)) return;

        if (_ownStreamBitmap is null
            || _ownStreamBitmap.PixelSize.Width != width
            || _ownStreamBitmap.PixelSize.Height != height)
        {
            _ownStreamBitmap = new WriteableBitmap(
                new PixelSize(width, height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Opaque);
            _ownStreamImage.Source = _ownStreamBitmap;
        }

        using (var frame = _ownStreamBitmap.Lock())
        {
            // Linha por linha: o bitmap pode ter bytes extras no fim de cada linha
            for (var y = 0; y < height; y++)
                Marshal.Copy(_previewBuffer, y * width * 4, frame.Address + y * frame.RowBytes, width * 4);
        }

        _ownStreamImage.InvalidateVisual();
    }

    /// A minha transmissão terminou: o quadrado some e a imagem antiga é descartada.
    private void ClearOwnStreamTile()
    {
        _ownStreamImage.Source = null;
        _ownStreamBitmap = null;
    }

    // ───────────────────────── Barra de baixo ─────────────────────────

    /// Monta a barra (chamado pelo InitSoundboard, depois de o botão 🎉 existir).
    private StackPanel BuildRoomBar()
    {
        // O 🎉 entra na barra: quem posiciona agora é ela
        _sfxButton.HorizontalAlignment = HorizontalAlignment.Center;
        _sfxButton.VerticalAlignment = VerticalAlignment.Center;
        _sfxButton.Margin = new Thickness(0);
        _sfxButton.IsVisible = true; // ele nascia escondido; agora quem aparece/some é a barra inteira

        // Câmera: ainda não existe (seção Camera do roadmap); fica apagada, sem clique
        _cameraButton = RoomBarButton(BarGlyph(CameraGlyph, RoomBarIconOff));
        _cameraButton.Cursor = Cursor.Default;
        ToolTip.SetTip(_cameraButton, Loc.T("Room_CameraSoon"));

        // Transmitir: abre o painel de escolha (iniciar, trocar a fonte ou parar)
        _shareIcon = BarGlyph(ScreenGlyph, RoomBarIconOff);
        _shareButton = RoomBarButton(_shareIcon);
        _shareButton.PointerEntered += (_, _) => SetRoomBarButtonHot(_shareButton, true);
        _shareButton.PointerExited += (_, _) => SetRoomBarButtonHot(_shareButton, false);
        _shareButton.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(_shareButton).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            SetSoundboardPanel(false);
            OpenSourcePicker();
        };

        _roomBar = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 12,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Bottom,
            Margin = new Thickness(0, 0, 0, VolumeEdgeMargin),
            ZIndex = 20,
            IsVisible = false,
        };
        _roomBar.Children.Add(_sfxButton);
        _roomBar.Children.Add(_cameraButton);
        _roomBar.Children.Add(_shareButton);

        UpdateRoomBar();
        return _roomBar;
    }

    /// Cor e dica do botão de transmitir (e da câmera, quando ela existir).
    private void UpdateRoomBar()
    {
        if (_shareButton is null) return;

        _shareIcon.Foreground = _streaming ? WatchedBorder : RoomBarIconOff;
        ToolTip.SetTip(_shareButton, Loc.T(_streaming ? "Stream_Change" : "Stream_Start"));
        SetRoomBarButtonHot(_shareButton, _shareButton.IsPointerOver);
    }

    /// Botão redondo da barra, no mesmo estilo do 🎉.
    private static Border RoomBarButton(Control icon) => new()
    {
        Width = SfxButtonSize,
        Height = SfxButtonSize,
        CornerRadius = new CornerRadius(SfxButtonSize / 2),
        Background = VolumeBackground,
        BorderBrush = VolumeBorder,
        BorderThickness = new Thickness(1),
        Cursor = new Cursor(StandardCursorType.Hand),
        Opacity = SfxIdleOpacity,
        RenderTransform = SfxScaleIdle,
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
        Child = icon,
    };

    private static TextBlock BarGlyph(string glyph, IBrush color) => new()
    {
        Text = glyph,
        FontFamily = IconFont,
        FontSize = 18,
        Foreground = color,
        HorizontalAlignment = HorizontalAlignment.Center,
        VerticalAlignment = VerticalAlignment.Center,
    };

    /// Aceso (mouse em cima): visível por inteiro e um pouco maior. Transmitindo, o botão de
    /// transmitir fica mais visível mesmo parado, para o verde aparecer.
    private void SetRoomBarButtonHot(Border button, bool hot)
    {
        var idle = ReferenceEquals(button, _shareButton) && _streaming ? 0.85 : SfxIdleOpacity;
        button.Opacity = hot ? 1 : idle;
        button.RenderTransform = hot ? SfxScaleHot : SfxScaleIdle;
    }
}