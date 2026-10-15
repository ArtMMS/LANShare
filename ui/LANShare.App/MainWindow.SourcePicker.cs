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

/// Painel "Escolha o monitor" (abre ao clicar em "Iniciar transmissão" no Host).
public partial class MainWindow
{
    /// De quanto em quanto tempo as prévias dos monitores são atualizadas.
    private static readonly TimeSpan ThumbnailRefresh = TimeSpan.FromSeconds(1);

    private static readonly IBrush AccentBrush = new SolidColorBrush(Color.Parse("#4F7CFF"));

    /// Um cartão de monitor e a imagem que ele mostra.
    private sealed class MonitorTile(int index, Image thumbnail, TextBlock placeholder, TextBlock resolution)
    {
        public int Index { get; } = index;
        public Image Thumbnail { get; } = thumbnail;
        public TextBlock Placeholder { get; } = placeholder;
        public TextBlock Resolution { get; } = resolution;
        public WriteableBitmap? Bitmap { get; set; }
    }

    /// Resultado de uma foto: tamanho da miniatura e do monitor.
    private readonly record struct Shot(int Width, int Height, int SourceWidth, int SourceHeight);

    private Border _sourcePicker = null!;
    private ListBox _monitorList = null!;
    private TextBlock _pickerStatus = null!;
    private Button _pickerConfirmButton = null!, _pickerCancelButton = null!;

    private bool _pickerOpen;

    /// Muda a cada abertura/fechamento: para a atualização das prévias do painel anterior.
    private int _pickerGeneration;

    /// Chamado uma vez no construtor.
    private void InitSourcePicker()
    {
        _sourcePicker = Find<Border>("SourcePicker");
        _monitorList = Find<ListBox>("MonitorList");
        _pickerStatus = Find<TextBlock>("PickerStatus");
        _pickerConfirmButton = Find<Button>("PickerConfirmButton");
        _pickerCancelButton = Find<Button>("PickerCancelButton");

        _pickerConfirmButton.Click += (_, _) => ConfirmSourcePicker();
        _pickerCancelButton.Click += (_, _) => CloseSourcePicker();
        _monitorList.DoubleTapped += (_, _) => ConfirmSourcePicker();
        _monitorList.SelectionChanged += (_, _) =>
            _pickerConfirmButton.IsEnabled = _monitorList.SelectedItem is not null;
        _monitorList.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            ConfirmSourcePicker();
            e.Handled = true;
        };

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

    /// Abre o painel por cima de tudo, com um cartão por monitor.
    private void OpenSourcePicker()
    {
        if (!_roomOpen || _streaming || _pickerOpen) return;

        var items = new List<ListBoxItem>();
        var tiles = new List<MonitorTile>();
        using (var monitors = NativeMethods.HostMonitors())
        {
            foreach (var monitor in monitors.RootElement.EnumerateArray())
            {
                var index = monitor.GetProperty("index").GetInt32();
                var item = BuildMonitorTile(
                    index,
                    monitor.GetProperty("name").GetString() ?? $"Monitor {index + 1}",
                    monitor.GetProperty("width").GetInt32(),
                    monitor.GetProperty("height").GetInt32(),
                    monitor.GetProperty("primary").GetBoolean());
                items.Add(item);
                tiles.Add((MonitorTile)item.Tag!);
            }
        }

        _pickerOpen = true;
        var generation = ++_pickerGeneration;

        _monitorList.ItemsSource = items;
        _monitorList.SelectedIndex = items.Count > 0 ? 0 : -1; // o principal já vem selecionado
        _pickerConfirmButton.IsEnabled = items.Count > 0;
        _pickerStatus.Text = items.Count == 0 ? "Nenhum monitor encontrado." : "";
        _pickerStatus.IsVisible = items.Count == 0;

        _sourcePicker.IsVisible = true;
        _mainLayout.IsEnabled = false; // o fundo fica travado enquanto o painel está aberto
        Dispatcher.UIThread.Post(() => _monitorList.Focus());

        _ = RefreshThumbnailsAsync(tiles, generation);
    }

    private void CloseSourcePicker()
    {
        if (!_pickerOpen) return;

        _pickerOpen = false;
        _pickerGeneration++; // para a atualização das prévias
        _sourcePicker.IsVisible = false;
        _mainLayout.IsEnabled = true;
        _monitorList.ItemsSource = null;
    }

    /// Fecha o painel e começa a transmitir o monitor escolhido.
    private void ConfirmSourcePicker()
    {
        if (!_pickerOpen) return;
        if (_monitorList.SelectedItem is not ListBoxItem { Tag: MonitorTile tile }) return;

        CloseSourcePicker();
        StartStream(tile.Index);
    }

    /// Atualiza a prévia de cada monitor, um por vez, enquanto o painel estiver aberto.
    private async Task RefreshThumbnailsAsync(List<MonitorTile> tiles, int generation)
    {
        var buffer = new byte[NativeMethods.ThumbnailBufferSize];

        while (generation == _pickerGeneration)
        {
            foreach (var tile in tiles)
            {
                Shot? shot;
                try
                {
                    shot = await Task.Run(() => TakeThumbnail(tile.Index, buffer));
                }
                catch (Exception)
                {
                    shot = null; // mantém o ícone (ou a última prévia) no cartão
                }

                // O painel pode ter sido fechado enquanto a foto era tirada
                if (generation != _pickerGeneration) return;

                if (shot is { } s) ShowThumbnail(tile, buffer, s);
            }

            await Task.Delay(ThumbnailRefresh);
        }
    }

    private static Shot? TakeThumbnail(int index, byte[] buffer) =>
        NativeMethods.HostMonitorThumbnail(index, buffer, out var w, out var h, out var sw, out var sh)
            ? new Shot(w, h, sw, sh)
            : null;

    /// Desenha a foto no cartão (reaproveita a imagem se o tamanho não mudou).
    private static void ShowThumbnail(MonitorTile tile, byte[] bgra, Shot shot)
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

        if (string.IsNullOrEmpty(tile.Resolution.Text))
            tile.Resolution.Text = $"{shot.SourceWidth} × {shot.SourceHeight}";
    }

    /// Cartão de um monitor: prévia, nome, resolução e o selo "Principal".
    private static ListBoxItem BuildMonitorTile(int index, string name, int width, int height, bool primary)
    {
        var thumbnail = new Image { Stretch = Stretch.Uniform };

        // Ícone de monitor enquanto a prévia não chega
        var placeholder = new TextBlock
        {
            Text = "\uE7F4",
            FontFamily = IconFont,
            FontSize = 34,
            Opacity = 0.35,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };

        var frame = new Border
        {
            Width = 220,
            Height = 124,
            CornerRadius = new CornerRadius(6),
            ClipToBounds = true,
            Background = new SolidColorBrush(Color.Parse("#0F1216")),
            Child = new Grid { Children = { thumbnail, placeholder } },
        };

        var label = new TextBlock
        {
            Text = name,
            FontWeight = FontWeight.SemiBold,
            TextAlignment = TextAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
            MaxWidth = 220,
        };
        ToolTip.SetTip(label, name);

        var resolution = new TextBlock
        {
            Text = width > 0 && height > 0 ? $"{width} × {height}" : "",
            FontSize = 11,
            Opacity = 0.7,
            VerticalAlignment = VerticalAlignment.Center,
        };

        var details = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            HorizontalAlignment = HorizontalAlignment.Center,
            Children = { resolution },
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

        var content = new StackPanel
        {
            Spacing = 8,
            Width = 220,
            Children = { frame, label, details },
        };

        var item = new ListBoxItem
        {
            Content = content,
            Tag = new MonitorTile(index, thumbnail, placeholder, resolution),
        };
        item.Classes.Add("monitor");
        return item;
    }
}