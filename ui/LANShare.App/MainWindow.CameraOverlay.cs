using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;

namespace LANShare.App;

/// Câmeras flutuantes (v2.0): com uma transmissão grande na tela (a de alguém, ou a prévia grande
/// da minha), as câmeras ligadas da sala (a versão de 720p, inclusive a minha) aparecem como
/// janelinhas por cima dela — também em tela cheia (F11). Só câmeras: quem está sem câmera não
/// aparece (nada de avatar).
///
/// - arrastar a janelinha move ela para qualquer lugar da área do vídeo;
/// - o canto de baixo, à direita, redimensiona (sempre em 16:9);
/// - a posição e o tamanho de cada câmera ficam guardados enquanto o app está aberto
///   (por pessoa); quem ainda não foi mexido fica empilhado no canto de cima, à direita.
///
/// As imagens são as mesmas dos quadrados da sala (`_ownCameraImage` e `_cameraTileViews`):
/// assistindo, elas saem da grade e entram aqui; parando de assistir, voltam para a grade.
/// A câmera que estou assistindo grande não aparece de novo como janelinha.
public partial class MainWindow
{
    private const double OverlayWidgetWidth = 240;
    private const double OverlayMinWidth = 120;
    private const double OverlayWidgetAspect = 16.0 / 9.0;
    private const double OverlayMargin = 16;
    private const double OverlayGap = 10;
    private const double OverlayResizeGrip = 18;

    private static readonly IBrush OverlayWidgetBackground = new SolidColorBrush(Color.Parse("#0B0D10"));
    private static readonly IBrush OverlayWidgetBorder = new SolidColorBrush(Color.Parse("#3A424C"));
    private static readonly IBrush OverlayWidgetBorderHot = new SolidColorBrush(Color.Parse("#7A8591"));
    private static readonly IBrush OverlayGripBrush = new SolidColorBrush(Color.Parse("#CCFFFFFF"));

    /// Uma janelinha e as peças que mudam depois de criada.
    private sealed class OverlayWidget
    {
        public required Border Root { get; init; }
        public required Grid Content { get; init; }
        public required TextBlock Name { get; init; }
        public required Control Grip { get; init; }
    }

    /// Arrastando (mover ou redimensionar) uma janelinha.
    private sealed class OverlayDrag
    {
        public required ulong UserId { get; init; }
        public required bool Resizing { get; init; }
        public required Point Start { get; init; }
        public required Rect StartRect { get; init; }
    }

    /// Onde as janelinhas ficam (por cima do vídeo, abaixo dos botões).
    private Canvas _cameraOverlay = null!;

    /// As janelinhas na tela, por pessoa.
    private readonly Dictionary<ulong, OverlayWidget> _overlayWidgets = new();

    /// Posição e tamanho que cada pessoa ganhou ao ser mexida (vale enquanto o app está aberto).
    private readonly Dictionary<ulong, Rect> _overlayRects = new();

    private OverlayDrag? _overlayDrag;

    /// Chamado uma vez (pelo InitRoomView).
    private void InitCameraOverlay()
    {
        _cameraOverlay = new Canvas
        {
            IsVisible = false,
            ZIndex = 10, // acima do vídeo; o volume e a barra de baixo (20) ficam por cima
            ClipToBounds = true,
        };

        // Mesmo Grid do vídeo: acompanha a área da transmissão (e a tela cheia)
        if (_previewImage.Parent is Panel stage) stage.Children.Add(_cameraOverlay);

        // A área mudou de tamanho (janela, tela cheia): reposiciona as janelinhas
        _cameraOverlay.SizeChanged += (_, _) => PlaceOverlayWidgets();

        // Arrastar (mover/redimensionar) é tratado na janela, no túnel e mesmo com o evento já
        // tratado: assim nada no caminho (nem a lista da sala sendo refeita) interrompe o arrasto.
        // O mouse fica preso à camada das câmeras, que nunca é recriada.
        AddHandler(PointerPressedEvent, OnOverlayPointerPressed, RoutingStrategies.Tunnel, handledEventsToo: true);
        AddHandler(PointerMovedEvent, OnOverlayPointerMoved, RoutingStrategies.Tunnel, handledEventsToo: true);
        AddHandler(PointerReleasedEvent, OnOverlayPointerReleased, RoutingStrategies.Tunnel, handledEventsToo: true);
        _cameraOverlay.PointerCaptureLost += (_, _) => EndOverlayDrag();
    }

    /// Clique numa janelinha: começa a mover (ou a redimensionar, se foi na alça do canto).
    private void OnOverlayPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!_cameraOverlay.IsVisible || _overlayDrag is not null || e.Source is not Visual source) return;
        if (!e.GetCurrentPoint(_cameraOverlay).Properties.IsLeftButtonPressed) return;

        foreach (var (id, widget) in _overlayWidgets)
        {
            if (!IsInside(source, widget.Root)) continue;

            _overlayDrag = new OverlayDrag
            {
                UserId = id,
                Resizing = IsInside(source, widget.Grip),
                Start = e.GetPosition(_cameraOverlay),
                StartRect = WidgetRect(widget.Root),
            };
            widget.Root.ZIndex = 1; // a que está sendo mexida fica por cima das outras
            SetOverlayWidgetHot(widget, true);
            e.Pointer.Capture(_cameraOverlay);
            e.Handled = true;
            return;
        }
    }

    private void OnOverlayPointerMoved(object? sender, PointerEventArgs e)
    {
        if (_overlayDrag is not { } drag) return;
        if (!_overlayWidgets.TryGetValue(drag.UserId, out var widget))
        {
            EndOverlayDrag(); // a câmera sumiu no meio do arrasto
            return;
        }

        var delta = e.GetPosition(_cameraOverlay) - drag.Start;
        var rect = drag.Resizing ? ResizeRect(drag.StartRect, delta.X) : drag.StartRect.Translate(delta);
        ApplyWidgetRect(widget.Root, ClampToOverlay(rect));
        e.Handled = true;
    }

    private void OnOverlayPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        if (_overlayDrag is null) return;
        EndOverlayDrag();
        e.Pointer.Capture(null);
        e.Handled = true;
    }

    /// Fim do arrasto: guarda o lugar e o tamanho dessa câmera para as próximas vezes.
    private void EndOverlayDrag()
    {
        if (_overlayDrag is not { } drag) return;
        _overlayDrag = null;

        if (!_overlayWidgets.TryGetValue(drag.UserId, out var widget)) return;
        widget.Root.ZIndex = 0;
        _overlayRects[drag.UserId] = WidgetRect(widget.Root);
        SetOverlayWidgetHot(widget, widget.Root.IsPointerOver);
    }

    /// Mouse em cima (ou arrastando): borda acesa e a alça de redimensionar visível.
    private static void SetOverlayWidgetHot(OverlayWidget widget, bool hot)
    {
        widget.Root.BorderBrush = hot ? OverlayWidgetBorderHot : OverlayWidgetBorder;
        widget.Grip.Opacity = hot ? 1 : 0;
    }

    /// As câmeras flutuantes aparecem? (Na sala, com uma transmissão grande na tela: a de alguém
    /// que estou assistindo ou conectando, ou a prévia grande da minha.)
    private bool CameraOverlayActive => InRoom && (_watchState != WatchState.None || _ownPreviewBig);

    /// Quais câmeras aparecem: a minha (ligada) primeiro, depois as dos outros que já têm imagem,
    /// menos a que estou assistindo grande.
    private List<(ulong Id, Image Image)> OverlayCameras()
    {
        var list = new List<(ulong, Image)>();
        if (!CameraOverlayActive) return list;

        if (_cameraOn) list.Add((MySfxUserId, _ownCameraImage));

        foreach (var id in _cameraUsers.OrderBy(id => id))
        {
            if (id == MySfxUserId) continue;
            if (_watchCamera && _watchUser == id) continue; // já está grande na tela
            if (_cameraTileViews.TryGetValue(id, out var view) && view.HasFrame) list.Add((id, view.Image));
        }

        return list;
    }

    /// Atualiza as janelinhas (chamado pelo LayoutRoomTiles, depois da grade).
    /// As que continuam na tela são as mesmas (não perdem o arrasto em andamento).
    private void RefreshCameraOverlay()
    {
        var cameras = OverlayCameras();
        var wanted = cameras.Select(c => c.Id).ToHashSet();

        // Quem saiu (câmera desligou, parou de assistir...)
        foreach (var id in _overlayWidgets.Keys.Where(id => !wanted.Contains(id)).ToList())
        {
            var widget = _overlayWidgets[id];
            widget.Content.Children.Clear(); // solta a imagem para a grade poder usar
            _cameraOverlay.Children.Remove(widget.Root);
            _overlayWidgets.Remove(id);
            if (_overlayDrag?.UserId == id) _overlayDrag = null;
        }

        foreach (var (id, image) in cameras)
        {
            if (!_overlayWidgets.TryGetValue(id, out var widget))
            {
                widget = BuildOverlayWidget(id);
                _overlayWidgets[id] = widget;
                _cameraOverlay.Children.Add(widget.Root);
            }

            // A imagem pode estar presa num quadrado da grade (ela é a mesma): traz para cá
            if (!ReferenceEquals(image.Parent, widget.Content))
            {
                if (image.Parent is Panel oldParent) oldParent.Children.Remove(image);
                widget.Content.Children.Insert(0, image);
            }

            widget.Name.Text = id == MySfxUserId ? Loc.T("Overlay_You") : UserName(id);
        }

        _cameraOverlay.IsVisible = _overlayWidgets.Count > 0;
        PlaceOverlayWidgets();
    }

    /// Uma janelinha: a câmera, o nome embaixo à esquerda e a alça de redimensionar no canto.
    private OverlayWidget BuildOverlayWidget(ulong userId)
    {
        var content = new Grid();

        var name = new TextBlock
        {
            FontSize = 12,
            TextTrimming = TextTrimming.CharacterEllipsis,
        };
        var nameTag = new Border
        {
            Background = RoomNameBackground,
            CornerRadius = new CornerRadius(5),
            Padding = new Thickness(6, 2),
            Margin = new Thickness(6, 6, OverlayResizeGrip + 4, 6),
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Bottom,
            IsHitTestVisible = false,
            Child = name,
        };

        // Alça de redimensionar: três tracinhos no canto de baixo, à direita
        var grip = new Panel
        {
            Width = OverlayResizeGrip,
            Height = OverlayResizeGrip,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Bottom,
            Background = Brushes.Transparent, // para receber o clique em toda a área
            Cursor = new Cursor(StandardCursorType.BottomRightCorner),
            Opacity = 0,
        };
        foreach (var inset in new[] { 4.0, 8.0, 12.0 })
        {
            grip.Children.Add(new Avalonia.Controls.Shapes.Line
            {
                StartPoint = new Point(OverlayResizeGrip - 3, inset),
                EndPoint = new Point(inset, OverlayResizeGrip - 3),
                Stroke = OverlayGripBrush,
                StrokeThickness = 1.5,
            });
        }

        // A imagem entra na posição 0 (embaixo); o nome e a alça ficam por cima
        content.Children.Add(nameTag);
        content.Children.Add(grip);

        var root = new Border
        {
            Width = OverlayWidgetWidth,
            Height = OverlayWidgetWidth / OverlayWidgetAspect,
            CornerRadius = new CornerRadius(10),
            Background = OverlayWidgetBackground,
            BorderBrush = OverlayWidgetBorder,
            BorderThickness = new Thickness(1),
            ClipToBounds = true,
            BoxShadow = BoxShadows.Parse("0 6 18 0 #99000000"),
            Cursor = new Cursor(StandardCursorType.SizeAll),
            Child = content,
        };

        var widget = new OverlayWidget { Root = root, Content = content, Name = name, Grip = grip };
        root.PointerEntered += (_, _) => SetOverlayWidgetHot(widget, true);
        root.PointerExited += (_, _) =>
        {
            if (_overlayDrag?.UserId == userId) return; // arrastando: continua aceso
            SetOverlayWidgetHot(widget, false);
        };

        return widget;
    }

    /// Posiciona as janelinhas: quem já foi mexido volta para o lugar guardado (ajustado à área);
    /// as outras ficam empilhadas no canto de cima, à direita (com mais colunas à esquerda
    /// se não couberem).
    private void PlaceOverlayWidgets()
    {
        var area = _cameraOverlay.Bounds.Size;
        if (area.Width <= 0 || area.Height <= 0) return;

        var height = OverlayWidgetWidth / OverlayWidgetAspect;
        var x = area.Width - OverlayMargin - OverlayWidgetWidth;
        var y = OverlayMargin;

        foreach (var (id, widget) in _overlayWidgets)
        {
            // Quem está sendo arrastado fica onde o mouse está
            if (_overlayDrag?.UserId == id) continue;

            if (_overlayRects.TryGetValue(id, out var saved))
            {
                ApplyWidgetRect(widget.Root, ClampToOverlay(saved));
                continue;
            }

            if (y + height > area.Height - OverlayMargin && y > OverlayMargin)
            {
                // Coluna cheia: começa outra à esquerda
                x -= OverlayWidgetWidth + OverlayGap;
                y = OverlayMargin;
            }

            ApplyWidgetRect(widget.Root, ClampToOverlay(new Rect(x, y, OverlayWidgetWidth, height)));
            y += height + OverlayGap;
        }
    }

    /// Novo tamanho puxando o canto: a largura muda com o mouse e a altura acompanha (16:9).
    private Rect ResizeRect(Rect start, double deltaX)
    {
        var area = _cameraOverlay.Bounds.Size;
        var maxWidth = Math.Max(OverlayMinWidth, Math.Min(area.Width - start.X, (area.Height - start.Y) * OverlayWidgetAspect));
        var width = Math.Clamp(start.Width + deltaX, OverlayMinWidth, maxWidth);
        return new Rect(start.X, start.Y, width, width / OverlayWidgetAspect);
    }

    /// Mantém a janelinha inteira dentro da área do vídeo (encolhendo, se a área ficou pequena).
    private Rect ClampToOverlay(Rect rect)
    {
        var area = _cameraOverlay.Bounds.Size;
        if (area.Width <= 0 || area.Height <= 0) return rect;

        var width = Math.Clamp(rect.Width, OverlayMinWidth, Math.Max(OverlayMinWidth, Math.Min(area.Width, area.Height * OverlayWidgetAspect)));
        var height = width / OverlayWidgetAspect;
        var x = Math.Clamp(rect.X, 0, Math.Max(0, area.Width - width));
        var y = Math.Clamp(rect.Y, 0, Math.Max(0, area.Height - height));
        return new Rect(x, y, width, height);
    }

    private static Rect WidgetRect(Border widget) =>
        new(Canvas.GetLeft(widget), Canvas.GetTop(widget), widget.Width, widget.Height);

    private static void ApplyWidgetRect(Border widget, Rect rect)
    {
        Canvas.SetLeft(widget, rect.X);
        Canvas.SetTop(widget, rect.Y);
        widget.Width = rect.Width;
        widget.Height = rect.Height;
    }
}