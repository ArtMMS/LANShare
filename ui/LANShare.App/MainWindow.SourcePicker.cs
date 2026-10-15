using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Layout;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;

namespace LANShare.App;

/// Janela "Escolha o monitor ou a janela" (aparece ao clicar em "Iniciar transmissão" no Host).
public partial class MainWindow
{
    /// O que cada cartão de monitor guarda, para a miniatura ser preenchida depois.
    private sealed record MonitorTile(int Index, Image Thumbnail, TextBlock Placeholder, TextBlock Resolution);

    private Border _sourcePicker = null!;
    private ListBox _monitorList = null!;
    private TextBlock _pickerStatus = null!;
    private Button _pickerConfirmButton = null!, _pickerCancelButton = null!;

    private bool _pickerOpen;

    /// Muda a cada abertura/fechamento: miniaturas que chegam atrasadas são descartadas.
    private int _pickerGeneration;

    /// Chamado uma vez no construtor, depois do InitializeComponent.
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

        // Tunnel: o Esc fecha a janela de qualquer ponto
        AddHandler(KeyDownEvent, OnPickerKeyDown, RoutingStrategies.Tunnel);
    }

    private void OnPickerKeyDown(object? sender, KeyEventArgs e)
    {
        if (!_pickerOpen || e.Key != Key.Escape) return;
        CloseSourcePicker();
        e.Handled = true;
    }

    /// Abre a janela por cima de tudo, com um cartão por monitor.
    private void OpenSourcePicker()
    {
        if (!_roomOpen || _streaming || _pickerOpen) return;

        var items = new List<ListBoxItem>();
        using (var monitors = NativeMethods.HostMonitors())
        {
            foreach (var monitor in monitors.RootElement.EnumerateArray())
            {
                var index = monitor.GetProperty("index").GetInt32();
                var name = monitor.GetProperty("name").GetString() ?? $"Monitor {index + 1}";
                items.Add(BuildMonitorTile(index, name));
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
        _mainLayout.IsEnabled = false; // o fundo fica travado enquanto a janela está aberta
        Dispatcher.UIThread.Post(() => _monitorList.Focus());

        _ = LoadMonitorThumbnailsAsync(items, generation);
    }

    private void CloseSourcePicker()
    {
        if (!_pickerOpen) return;

        _pickerOpen = false;
        _pickerGeneration++;
        _sourcePicker.IsVisible = false;
        _mainLayout.IsEnabled = true;
        _monitorList.ItemsSource = null;
    }

    /// Fecha a janela e começa a transmitir o monitor escolhido.
    private void ConfirmSourcePicker()
    {
        if (!_pickerOpen) return;
        if (_monitorList.SelectedItem is not ListBoxItem { Tag: MonitorTile tile }) return;

        CloseSourcePicker();
        StartStream(tile.Index);
    }

    /// Tira a foto de cada monitor, um por vez, sem travar a janela.
    private async Task LoadMonitorThumbnailsAsync(List<ListBoxItem> items, int generation)
    {
        foreach (var item in items)
        {
            if (generation != _pickerGeneration || item.Tag is not MonitorTile tile) return;

            try
            {
                var result = await Task.Run<(byte[]? Buffer, int W, int H, int SourceW, int SourceH)>(() =>
                {
                    var buffer = new byte[NativeMethods.ThumbnailBufferSize];
                    var ok = NativeMethods.HostMonitorThumbnail(
                        tile.Index, buffer, out var w, out var h, out var sw, out var sh);
                    return ok ? (buffer, w, h, sw, sh) : (null, 0, 0, 0, 0);
                });

                // A janela pode ter sido fechada enquanto a foto era tirada
                if (generation != _pickerGeneration) return;

                if (result.Buffer is null)
                {
                    tile.Resolution.Text = "Pré-visualização indisponível";
                    continue;
                }

                tile.Thumbnail.Source = MakeBitmap(result.Buffer, result.W, result.H);
                tile.Placeholder.IsVisible = false;
                tile.Resolution.Text = $"{result.SourceW} × {result.SourceH}";
            }
            catch (Exception ex)
            {
                tile.Resolution.Text = "Pré-visualização indisponível";
                _infoText.Text = "Erro ao capturar o monitor: " + ex.Message;
            }
        }
    }

    /// Cartão de um monitor: miniatura, nome e resolução.
    private static ListBoxItem BuildMonitorTile(int index, string name)
    {
        var thumbnail = new Image { Stretch = Avalonia.Media.Stretch.Uniform };

        // Ícone de monitor enquanto a foto não chega
        var placeholder = new TextBlock
        {
            Text = "\uE7F4",
            FontFamily = IconFont,
            FontSize = 30,
            Opacity = 0.35,
            HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center,
        };

        var frame = new Border
        {
            Width = 160,
            Height = 90,
            CornerRadius = new CornerRadius(6),
            ClipToBounds = true,
            Background = new Avalonia.Media.SolidColorBrush(Avalonia.Media.Color.Parse("#0F1216")),
            Child = new Grid { Children = { thumbnail, placeholder } },
        };

        var label = new TextBlock
        {
            Text = name,
            FontWeight = Avalonia.Media.FontWeight.SemiBold,
            TextAlignment = Avalonia.Media.TextAlignment.Center,
            TextTrimming = Avalonia.Media.TextTrimming.CharacterEllipsis,
            MaxWidth = 160,
        };

        var resolution = new TextBlock
        {
            Text = "Carregando...",
            FontSize = 11,
            Opacity = 0.7,
            TextAlignment = Avalonia.Media.TextAlignment.Center,
        };

        var content = new StackPanel
        {
            Spacing = 6,
            HorizontalAlignment = HorizontalAlignment.Center,
            Children = { frame, label, resolution },
        };

        var item = new ListBoxItem
        {
            Content = content,
            Tag = new MonitorTile(index, thumbnail, placeholder, resolution),
        };
        item.Classes.Add("room"); // mesmo visual dos cartões de sala
        return item;
    }

    /// Cria a imagem a partir de pixels BGRA.
    private static WriteableBitmap MakeBitmap(byte[] bgra, int width, int height)
    {
        var bitmap = new WriteableBitmap(
            new PixelSize(width, height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Opaque);

        using var frame = bitmap.Lock();
        for (var y = 0; y < height; y++)
            Marshal.Copy(bgra, y * width * 4, frame.Address + y * frame.RowBytes, width * 4);

        return bitmap;
    }
}