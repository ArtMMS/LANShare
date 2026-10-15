using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;

namespace LANShare.App;

/// Painel de escolha do que transmitir (abre em "Iniciar transmissão" e em "Alterar transmissão").
/// Duas abas: "Monitores inteiros" e "Janela específica".
public partial class MainWindow
{
    /// Pausa entre uma volta de atualização das prévias e a próxima.
    private static readonly TimeSpan ThumbnailRefresh = TimeSpan.FromSeconds(1);

    /// Tamanho da prévia em cada cartão.
    private const double TileWidth = 180;
    private const double TileHeight = 102;

    private static readonly IBrush AccentBrush = new SolidColorBrush(Color.Parse("#4F7CFF"));

    private enum PickerTab { Monitors, Windows }

    /// Um cartão (monitor ou janela) e as peças que mudam depois de criado.
    private sealed class SourceTile
    {
        public int MonitorIndex { get; init; } = -1;
        public ulong WindowId { get; init; }
        public bool IsWindow => WindowId != 0;
        public bool Minimized { get; init; }

        public required Image Thumbnail { get; init; }
        public required TextBlock Placeholder { get; init; }
        public required Image BigIcon { get; init; }
        public required TextBlock Detail { get; init; }
        public Image? SmallIcon { get; init; }

        public WriteableBitmap? Bitmap { get; set; }
    }

    /// Resultado de uma foto: tamanho da miniatura e do que foi capturado.
    private readonly record struct Shot(int Width, int Height, int SourceWidth, int SourceHeight);

    private Border _sourcePicker = null!;
    private ListBox _monitorList = null!, _windowList = null!;
    private TextBlock _pickerStatus = null!;
    private Button _monitorsTabButton = null!, _windowsTabButton = null!;
    private Button _pickerConfirmButton = null!, _pickerCancelButton = null!;

    private bool _pickerOpen;
    private PickerTab _pickerTab = PickerTab.Monitors;
    private List<SourceTile> _monitorTiles = new(), _windowTiles = new();

    /// Muda a cada abertura/fechamento: para as tarefas do painel anterior.
    private int _pickerGeneration;

    /// O que está sendo transmitido agora (para o painel já abrir com ele marcado).
    private int _streamMonitor = -1;
    private ulong _streamWindow;

    private ListBox ActivePickerList => _pickerTab == PickerTab.Monitors ? _monitorList : _windowList;

    /// Chamado uma vez no construtor.
    private void InitSourcePicker()
    {
        _sourcePicker = Find<Border>("SourcePicker");
        _monitorList = Find<ListBox>("MonitorList");
        _windowList = Find<ListBox>("WindowList");
        _pickerStatus = Find<TextBlock>("PickerStatus");
        _monitorsTabButton = Find<Button>("MonitorsTabButton");
        _windowsTabButton = Find<Button>("WindowsTabButton");
        _pickerConfirmButton = Find<Button>("PickerConfirmButton");
        _pickerCancelButton = Find<Button>("PickerCancelButton");

        _monitorsTabButton.Click += (_, _) => SelectPickerTab(PickerTab.Monitors);
        _windowsTabButton.Click += (_, _) => SelectPickerTab(PickerTab.Windows);
        _pickerConfirmButton.Click += (_, _) => ConfirmSourcePicker();
        _pickerCancelButton.Click += (_, _) => CloseSourcePicker();

        foreach (var list in new[] { _monitorList, _windowList })
        {
            list.DoubleTapped += (_, _) => ConfirmSourcePicker();
            list.SelectionChanged += (_, _) => UpdatePickerConfirm();
            list.KeyDown += (_, e) =>
            {
                if (e.Key != Key.Enter) return;
                ConfirmSourcePicker();
                e.Handled = true;
            };
        }

        // Clicar na parte escurecida (fora do painel) cancela
        _sourcePicker.PointerPressed += (_, e) =>
        {
            if (ReferenceEquals(e.Source, _sourcePicker)) CloseSourcePicker();
        };

        // Tunnel: o Esc fecha o painel de qualquer ponto
        AddHandler(KeyDownEvent, OnPickerKeyDown, RoutingStrategies.Tunnel);
    }

    private void OnPickerKeyDown(object? sender, KeyEventArgs e)
    {
        if (!_pickerOpen || e.Key != Key.Escape) return;
        CloseSourcePicker();
        e.Handled = true;
    }

    /// Abre o painel por cima de tudo. Transmitindo, já abre com a fonte atual marcada.
    private void OpenSourcePicker()
    {
        if (!_roomOpen || _pickerOpen) return;

        _pickerOpen = true;
        var generation = ++_pickerGeneration;

        var monitorItems = BuildMonitorItems();
        var windowItems = BuildWindowItems();
        _monitorTiles = monitorItems.Select(i => (SourceTile)i.Tag!).ToList();
        _windowTiles = windowItems.Select(i => (SourceTile)i.Tag!).ToList();

        _monitorList.ItemsSource = monitorItems;
        _windowList.ItemsSource = windowItems;
        _monitorList.SelectedIndex = monitorItems.Count > 0 ? 0 : -1; // o principal já vem selecionado
        _windowList.SelectedIndex = -1; // a janela o Host escolhe

        var startTab = PickerTab.Monitors;
        if (_streaming && _streamWindow != 0)
        {
            var current = windowItems.FirstOrDefault(i => ((SourceTile)i.Tag!).WindowId == _streamWindow);
            if (current is not null)
            {
                _windowList.SelectedItem = current;
                startTab = PickerTab.Windows;
            }
        }
        else if (_streaming && _streamMonitor >= 0)
        {
            var current = monitorItems.FirstOrDefault(i => ((SourceTile)i.Tag!).MonitorIndex == _streamMonitor);
            if (current is not null) _monitorList.SelectedItem = current;
        }

        SelectPickerTab(startTab);
        _sourcePicker.IsVisible = true;
        _mainLayout.IsEnabled = false; // o fundo fica travado enquanto o painel está aberto

        _ = RefreshThumbnailsAsync(generation);
        _ = LoadWindowIconsAsync(_windowTiles, generation);
    }

    private void CloseSourcePicker()
    {
        if (!_pickerOpen) return;

        _pickerOpen = false;
        _pickerGeneration++; // para a atualização das prévias e dos ícones
        _sourcePicker.IsVisible = false;
        _mainLayout.IsEnabled = true;
        _monitorList.ItemsSource = null;
        _windowList.ItemsSource = null;
        _monitorTiles = new();
        _windowTiles = new();
    }

    private void SelectPickerTab(PickerTab tab)
    {
        _pickerTab = tab;
        var monitors = tab == PickerTab.Monitors;

        _monitorsTabButton.Classes.Set("active", monitors);
        _windowsTabButton.Classes.Set("active", !monitors);
        _monitorList.IsVisible = monitors;
        _windowList.IsVisible = !monitors;

        var empty = monitors ? _monitorTiles.Count == 0 : _windowTiles.Count == 0;
        _pickerStatus.Text = !empty ? "" : monitors
            ? "Nenhum monitor encontrado."
            : "Nenhuma janela aberta para transmitir.";
        _pickerStatus.IsVisible = empty;

        UpdatePickerConfirm();
        Dispatcher.UIThread.Post(() => ActivePickerList.Focus());
    }

    /// Texto e estado do botão de confirmar (Iniciar ou Alterar, conforme a transmissão).
    private void UpdatePickerConfirm()
    {
        _pickerConfirmButton.Content = _streaming ? "Alterar transmissão" : "Iniciar transmissão";
        _pickerConfirmButton.IsEnabled = ActivePickerList.SelectedItem is not null;
    }

    /// Fecha o painel e começa (ou troca) a transmissão para o que foi escolhido.
    private void ConfirmSourcePicker()
    {
        if (!_pickerOpen) return;
        if (ActivePickerList.SelectedItem is not ListBoxItem { Tag: SourceTile tile }) return;

        CloseSourcePicker();

        if (_streaming)
        {
            SwitchStream(tile);
            return;
        }

        if (tile.IsWindow) StartWindowStream(tile.WindowId);
        else StartStream(tile.MonitorIndex);

        if (_streaming) RememberSource(tile);
    }

    /// Troca o monitor/janela sem parar a transmissão.
    private void SwitchStream(SourceTile tile)
    {
        // Escolheu o mesmo que já está no ar: nada a fazer
        if (tile.IsWindow ? tile.WindowId == _streamWindow : tile.MonitorIndex == _streamMonitor) return;

        var (ok, error) = tile.IsWindow
            ? NativeMethods.HostSwitchWindow(tile.WindowId)
            : NativeMethods.HostSwitchMonitor(tile.MonitorIndex);
        if (!ok)
        {
            _infoText.Text = error ?? "Não foi possível alterar a transmissão.";
            return;
        }

        RememberSource(tile);
        _infoText.Text = tile.IsWindow ? "Transmissão alterada para a janela." : "Transmissão alterada para o monitor.";
    }

    private void RememberSource(SourceTile tile)
    {
        _streamWindow = tile.IsWindow ? tile.WindowId : 0;
        _streamMonitor = tile.IsWindow ? -1 : tile.MonitorIndex;
    }

    /// Começa a transmitir uma janela (o monitor usa o StartStream do MainWindow.axaml.cs).
    private void StartWindowStream(ulong windowId)
    {
        var height = _resolutionBox.SelectedIndex == 0 ? 720u : 1080u;
        var fps = _fpsBox.SelectedIndex == 0 ? 15u : 30u;
        var (ok, error) = NativeMethods.HostStartStreamWindow(height, fps, windowId);
        if (!ok)
        {
            _infoText.Text = error ?? "Erro ao iniciar a transmissão.";
            return;
        }

        _streaming = true;
        _infoText.Text = "Capturando a janela. A imagem é enviada quando um usuário entrar.";
        UpdateUi();
    }

    // ───────────────────────── Prévias e ícones ─────────────────────────

    /// Atualiza as prévias da aba aberta, uma por vez, enquanto o painel estiver aberto.
    private async Task RefreshThumbnailsAsync(int generation)
    {
        var buffer = new byte[NativeMethods.ThumbnailBufferSize];

        while (generation == _pickerGeneration)
        {
            var tab = _pickerTab;
            var tiles = tab == PickerTab.Monitors ? _monitorTiles : _windowTiles;

            foreach (var tile in tiles)
            {
                if (generation != _pickerGeneration) return;
                if (_pickerTab != tab) break; // trocou de aba: começa a outra já
                if (tile.Minimized) continue; // janela minimizada não tem imagem

                Shot? shot;
                try
                {
                    shot = await Task.Run(() => TakeThumbnail(tile, buffer));
                }
                catch (Exception)
                {
                    shot = null; // mantém o que o cartão já mostra
                }

                // O painel pode ter sido fechado enquanto a foto era tirada
                if (generation != _pickerGeneration) return;

                if (shot is { } s) ShowThumbnail(tile, buffer, s);
            }

            if (_pickerTab == tab) await Task.Delay(ThumbnailRefresh);
        }
    }

    private static Shot? TakeThumbnail(SourceTile tile, byte[] buffer)
    {
        int w, h, sw, sh;
        var ok = tile.IsWindow
            ? NativeMethods.HostWindowThumbnail(tile.WindowId, buffer, out w, out h, out sw, out sh)
            : NativeMethods.HostMonitorThumbnail(tile.MonitorIndex, buffer, out w, out h, out sw, out sh);
        return ok ? new Shot(w, h, sw, sh) : null;
    }

    /// Desenha a foto no cartão (reaproveita a imagem se o tamanho não mudou).
    private static void ShowThumbnail(SourceTile tile, byte[] bgra, Shot shot)
    {
        if (tile.Bitmap is null
            || tile.Bitmap.PixelSize.Width != shot.Width
            || tile.Bitmap.PixelSize.Height != shot.Height)
        {
            tile.Bitmap = new WriteableBitmap(
                new PixelSize(shot.Width, shot.Height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Opaque);
            tile.Thumbnail.Source = tile.Bitmap;
        }

        using (var frame = tile.Bitmap.Lock())
        {
            for (var y = 0; y < shot.Height; y++)
                Marshal.Copy(bgra, y * shot.Width * 4, frame.Address + y * frame.RowBytes, shot.Width * 4);
        }

        tile.Thumbnail.InvalidateVisual();
        tile.Placeholder.IsVisible = false;

        if (!tile.IsWindow && string.IsNullOrEmpty(tile.Detail.Text))
            tile.Detail.Text = $"{shot.SourceWidth} × {shot.SourceHeight}";
    }

    /// Busca o ícone de cada janela (uma por vez, fora da thread da interface).
    private async Task LoadWindowIconsAsync(List<SourceTile> tiles, int generation)
    {
        foreach (var tile in tiles)
        {
            if (generation != _pickerGeneration) return;

            byte[]? pixels;
            try
            {
                pixels = await Task.Run(() =>
                {
                    var buffer = new byte[NativeMethods.WindowIconBufferSize];
                    return NativeMethods.HostWindowIcon(tile.WindowId, buffer) ? buffer : null;
                });
            }
            catch (Exception)
            {
                pixels = null;
            }

            if (generation != _pickerGeneration) return;
            if (pixels is null) continue;

            var icon = MakeIconBitmap(pixels);
            if (tile.SmallIcon is not null)
            {
                tile.SmallIcon.Source = icon;
                tile.SmallIcon.IsVisible = true;
            }

            // Minimizada: o ícone grande ocupa o lugar da prévia
            if (tile.Minimized)
            {
                tile.BigIcon.Source = icon;
                tile.BigIcon.IsVisible = true;
                tile.Placeholder.IsVisible = false;
            }
        }
    }

    private static WriteableBitmap MakeIconBitmap(byte[] bgra)
    {
        var size = NativeMethods.WindowIconSize;
        var bitmap = new WriteableBitmap(
            new PixelSize(size, size), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Premul);

        using var frame = bitmap.Lock();
        for (var y = 0; y < size; y++)
            Marshal.Copy(bgra, y * size * 4, frame.Address + y * frame.RowBytes, size * 4);

        return bitmap;
    }

    // ───────────────────────── Cartões ─────────────────────────

    private static List<ListBoxItem> BuildMonitorItems()
    {
        var items = new List<ListBoxItem>();
        using var monitors = NativeMethods.HostMonitors();
        foreach (var m in monitors.RootElement.EnumerateArray())
        {
            var index = m.GetProperty("index").GetInt32();
            items.Add(BuildMonitorTile(
                index,
                m.GetProperty("name").GetString() ?? $"Monitor {index + 1}",
                m.GetProperty("width").GetInt32(),
                m.GetProperty("height").GetInt32(),
                m.GetProperty("primary").GetBoolean()));
        }
        return items;
    }

    private static List<ListBoxItem> BuildWindowItems()
    {
        var items = new List<ListBoxItem>();
        using var windows = NativeMethods.HostWindows();
        foreach (var w in windows.RootElement.EnumerateArray())
        {
            items.Add(BuildWindowTile(
                w.GetProperty("id").GetUInt64(),
                w.GetProperty("title").GetString() ?? "Janela",
                w.GetProperty("minimized").GetBoolean()));
        }
        return items;
    }

    /// Cartão de monitor: prévia, nome, resolução e o selo "Principal".
    private static ListBoxItem BuildMonitorTile(int index, string name, int width, int height, bool primary)
    {
        var (frame, thumbnail, placeholder, bigIcon) = BuildPreviewFrame("\uE7F4");

        var label = TileTitle(name);
        label.MaxWidth = TileWidth;

        var detail = TileDetail(width > 0 && height > 0 ? $"{width} × {height}" : "");
        var details = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            HorizontalAlignment = HorizontalAlignment.Center,
            MinHeight = 16,
            Children = { detail },
        };
        if (primary)
        {
            details.Children.Add(new Border
            {
                Background = AccentBrush,
                CornerRadius = new CornerRadius(4),
                Padding = new Thickness(6, 1),
                VerticalAlignment = VerticalAlignment.Center,
                Child = new TextBlock
                {
                    Text = "Principal",
                    FontSize = 10,
                    FontWeight = FontWeight.SemiBold,
                    Foreground = Brushes.White,
                },
            });
        }

        var tile = new SourceTile
        {
            MonitorIndex = index,
            Thumbnail = thumbnail,
            Placeholder = placeholder,
            BigIcon = bigIcon,
            Detail = detail,
        };
        return WrapTile(tile, frame, label, details);
    }

    /// Cartão de janela: prévia (ou ícone grande, se minimizada), ícone + título.
    private static ListBoxItem BuildWindowTile(ulong id, string title, bool minimized)
    {
        var (frame, thumbnail, placeholder, bigIcon) = BuildPreviewFrame("\uE7C4");

        var smallIcon = new Image
        {
            Width = 18,
            Height = 18,
            IsVisible = false,
            VerticalAlignment = VerticalAlignment.Center,
        };
        var titleText = TileTitle(title);
        titleText.MaxWidth = TileWidth - 26;

        var label = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            HorizontalAlignment = HorizontalAlignment.Center,
            Children = { smallIcon, titleText },
        };

        var detail = TileDetail(minimized ? "Minimizada" : "");
        var details = new StackPanel
        {
            HorizontalAlignment = HorizontalAlignment.Center,
            MinHeight = 16,
            Children = { detail },
        };

        var tile = new SourceTile
        {
            WindowId = id,
            Minimized = minimized,
            Thumbnail = thumbnail,
            Placeholder = placeholder,
            BigIcon = bigIcon,
            Detail = detail,
            SmallIcon = smallIcon,
        };
        return WrapTile(tile, frame, label, details);
    }

    /// Quadro da prévia: a imagem, o ícone de espera e o ícone grande (janela minimizada).
    private static (Border Frame, Image Thumbnail, TextBlock Placeholder, Image BigIcon) BuildPreviewFrame(string glyph)
    {
        var thumbnail = new Image { Stretch = Stretch.Uniform };

        var placeholder = new TextBlock
        {
            Text = glyph,
            FontFamily = IconFont,
            FontSize = 30,
            Opacity = 0.35,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };

        var bigIcon = new Image
        {
            Width = 40,
            Height = 40,
            IsVisible = false,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };

        var frame = new Border
        {
            Width = TileWidth,
            Height = TileHeight,
            CornerRadius = new CornerRadius(6),
            ClipToBounds = true,
            Background = new SolidColorBrush(Color.Parse("#0F1216")),
            Child = new Grid { Children = { thumbnail, placeholder, bigIcon } },
        };

        return (frame, thumbnail, placeholder, bigIcon);
    }

    private static TextBlock TileTitle(string text)
    {
        var label = new TextBlock
        {
            Text = text,
            FontWeight = FontWeight.SemiBold,
            TextAlignment = TextAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            VerticalAlignment = VerticalAlignment.Center,
        };
        ToolTip.SetTip(label, text);
        return label;
    }

    private static TextBlock TileDetail(string text) => new()
    {
        Text = text,
        FontSize = 11,
        Opacity = 0.7,
        VerticalAlignment = VerticalAlignment.Center,
    };

    private static ListBoxItem WrapTile(SourceTile tile, Control frame, Control label, Control details)
    {
        var content = new StackPanel
        {
            Spacing = 8,
            Width = TileWidth,
            Children = { frame, label, details },
        };

        var item = new ListBoxItem { Content = content, Tag = tile };
        item.Classes.Add("source");
        return item;
    }
}