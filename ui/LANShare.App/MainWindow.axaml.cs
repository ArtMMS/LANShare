using System.Runtime.InteropServices;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Markup.Xaml;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;

namespace LANShare.App;

public partial class MainWindow : Window
{
    private readonly TextBlock _versionText, _placeholderText, _infoText, _statusLabel, _addressLabel;
    private readonly TextBox _roomNameBox, _passwordBox;
    private readonly Button _roomButton, _startStreamButton, _stopStreamButton;
    private readonly ComboBox _resolutionBox, _fpsBox;
    private readonly StackPanel _usersPanel;
    private readonly Image _previewImage;
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(100) };

    // Buffer reaproveitado para receber a miniatura do Rust
    private readonly byte[] _previewBuffer = new byte[NativeMethods.PreviewBufferSize];
    private WriteableBitmap? _previewBitmap;

    private bool _roomOpen;
    private bool _streaming;
    private bool _coreOk;

    public MainWindow()
    {
        InitializeComponent();

        _versionText = this.FindControl<TextBlock>("VersionText")!;
        _placeholderText = this.FindControl<TextBlock>("PlaceholderText")!;
        _infoText = this.FindControl<TextBlock>("InfoText")!;
        _statusLabel = this.FindControl<TextBlock>("StatusLabel")!;
        _addressLabel = this.FindControl<TextBlock>("AddressLabel")!;
        _roomNameBox = this.FindControl<TextBox>("RoomNameBox")!;
        _passwordBox = this.FindControl<TextBox>("PasswordBox")!;
        _roomButton = this.FindControl<Button>("RoomButton")!;
        _startStreamButton = this.FindControl<Button>("StartStreamButton")!;
        _stopStreamButton = this.FindControl<Button>("StopStreamButton")!;
        _resolutionBox = this.FindControl<ComboBox>("ResolutionBox")!;
        _fpsBox = this.FindControl<ComboBox>("FpsBox")!;
        _usersPanel = this.FindControl<StackPanel>("UsersPanel")!;
        _previewImage = this.FindControl<Image>("PreviewImage")!;

        _roomButton.Click += (_, _) => ToggleRoom();
        _startStreamButton.Click += (_, _) => StartStream();
        _stopStreamButton.Click += (_, _) => NativeMethods.HostStopStream();
        _timer.Tick += (_, _) => OnTick();

        ShowCoreStatus();
        Closing += (_, _) => { if (_coreOk) NativeMethods.HostStop(); };
    }

    private void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    /// Confere se a ponte C# ↔ Rust funciona.
    private void ShowCoreStatus()
    {
        try
        {
            _versionText.Text = $"Motor Rust v{NativeMethods.GetVersion()}";
            _placeholderText.Text = "Abra a sala para começar.";
            _coreOk = true;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            _versionText.Text = "Motor Rust indisponível";
            _placeholderText.Text = "Problema com a lanshare_ffi.dll. Rode: cargo build --release -p lanshare-ffi";
            _roomButton.IsEnabled = false;
        }
    }

    private void ToggleRoom()
    {
        if (_roomOpen)
        {
            NativeMethods.HostStop();
            _timer.Stop();
            _roomOpen = false;
            _streaming = false;
            _usersPanel.Children.Clear();
            _addressLabel.Text = "";
            _infoText.Text = "";
            HidePreview("Sala fechada.");
            UpdateUi();
            return;
        }

        var (ok, error) = NativeMethods.HostStart(_passwordBox.Text, null, _roomNameBox.Text);
        if (!ok)
        {
            _placeholderText.Text = error ?? "Erro ao abrir a sala.";
            return;
        }

        _roomOpen = true;
        _timer.Start();
        UpdateUi();
    }

    private void StartStream()
    {
        var height = _resolutionBox.SelectedIndex == 0 ? 720u : 1080u;
        var fps = _fpsBox.SelectedIndex == 0 ? 15u : 30u;
        var (ok, error) = NativeMethods.HostStartStream(height, fps);
        if (!ok)
        {
            _infoText.Text = error ?? "Erro ao iniciar a transmissão.";
            return;
        }

        _streaming = true;
        _infoText.Text = "Capturando a tela. A imagem é enviada quando um usuário entrar.";
        UpdateUi();
    }

    /// A cada 100 ms: lê os eventos do Rust e atualiza o preview.
    private void OnTick()
    {
        try
        {
            PollEvents();
            if (_roomOpen && _streaming) UpdatePreview();
        }
        catch (Exception ex)
        {
            _infoText.Text = "Erro: " + ex.Message;
        }
    }

    private void PollEvents()
    {
        var changedUsers = false;
        while (NativeMethods.HostPollEvent() is { } doc)
        {
            using (doc)
            {
                var e = doc.RootElement;
                switch (e.GetProperty("type").GetString())
                {
                    case "started":
                        _addressLabel.Text = "Endereço: " + e.GetProperty("address").GetString();
                        _placeholderText.Text = "Sala aberta. Clique em Iniciar transmissão.";
                        break;
                    case "user_joined":
                    case "user_left":
                    case "user_kicked":
                        changedUsers = true;
                        break;
                    case "stream_started":
                        _infoText.Text =
                            $"Transmitindo {e.GetProperty("width").GetInt32()}x{e.GetProperty("height").GetInt32()} " +
                            $"@ {e.GetProperty("fps").GetInt32()} FPS";
                        break;
                    case "stream_stats":
                        _infoText.Text =
                            $"Transmitindo — {e.GetProperty("mbps").GetDouble():F1} Mbps, " +
                            $"{e.GetProperty("clients").GetInt32()} usuário(s)";
                        break;
                    case "stream_stopped":
                        _streaming = false;
                        _infoText.Text = e.TryGetProperty("error", out var err) && err.ValueKind == JsonValueKind.String
                            ? "Transmissão interrompida: " + err.GetString()
                            : "";
                        HidePreview("Transmissão parada.");
                        UpdateUi();
                        break;
                }
            }
        }

        if (changedUsers) RefreshUsers();
    }

    /// Pede a miniatura mais recente ao Rust e desenha na área do vídeo.
    private void UpdatePreview()
    {
        if (!NativeMethods.HostPreview(_previewBuffer, out var width, out var height)) return;

        if (_previewBitmap is null || _previewBitmap.PixelSize.Width != width || _previewBitmap.PixelSize.Height != height)
        {
            _previewBitmap = new WriteableBitmap(
                new PixelSize(width, height), new Vector(96, 96), PixelFormat.Bgra8888, AlphaFormat.Opaque);
            _previewImage.Source = _previewBitmap;
        }

        using (var frame = _previewBitmap.Lock())
        {
            // Copia linha por linha: o bitmap pode ter bytes extras no fim de cada linha
            for (var y = 0; y < height; y++)
                Marshal.Copy(_previewBuffer, y * width * 4, frame.Address + y * frame.RowBytes, width * 4);
        }

        // Força o redesenho com o conteúdo novo
        _previewImage.InvalidateVisual();
        _previewImage.IsVisible = true;
        _placeholderText.IsVisible = false;
    }

    private void HidePreview(string message)
    {
        _previewImage.IsVisible = false;
        _previewImage.Source = null;
        _previewBitmap = null;
        _placeholderText.Text = message;
        _placeholderText.IsVisible = true;
    }

    private void RefreshUsers()
    {
        _usersPanel.Children.Clear();
        using var users = NativeMethods.HostUsers();
        foreach (var user in users.RootElement.EnumerateArray())
        {
            var id = user.GetProperty("id").GetUInt64();
            var name = user.GetProperty("name").GetString();
            var kick = new Button { Content = "Expulsar" };
            kick.Click += (_, _) => NativeMethods.HostKick(id);

            var row = new DockPanel();
            DockPanel.SetDock(kick, Dock.Right);
            row.Children.Add(kick);
            row.Children.Add(new TextBlock { Text = name, VerticalAlignment = Avalonia.Layout.VerticalAlignment.Center });
            _usersPanel.Children.Add(row);
        }
    }

    private void UpdateUi()
    {
        _statusLabel.Text = _roomOpen ? (_streaming ? "Transmitindo" : "Sala aberta") : "Sala fechada";
        _roomButton.Content = _roomOpen ? "Fechar sala" : "Abrir sala";
        _roomNameBox.IsEnabled = _passwordBox.IsEnabled = !_roomOpen;
        _startStreamButton.IsEnabled = _roomOpen && !_streaming;
        _stopStreamButton.IsEnabled = _roomOpen && _streaming;
    }
}