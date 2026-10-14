using System.Text.Json;
using Avalonia.Controls;
using Avalonia.Markup.Xaml;
using Avalonia.Threading;

namespace LANShare.App;

public partial class MainWindow : Window
{
    private readonly TextBlock _versionText, _statusText, _statusLabel, _addressLabel;
    private readonly TextBox _roomNameBox, _passwordBox;
    private readonly Button _roomButton, _startStreamButton, _stopStreamButton;
    private readonly ComboBox _resolutionBox, _fpsBox;
    private readonly StackPanel _usersPanel;
    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(250) };

    private bool _roomOpen;
    private bool _streaming;
    private bool _coreOk;

    public MainWindow()
    {
        InitializeComponent();

        _versionText = this.FindControl<TextBlock>("VersionText")!;
        _statusText = this.FindControl<TextBlock>("StatusText")!;
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

        _roomButton.Click += (_, _) => ToggleRoom();
        _startStreamButton.Click += (_, _) => StartStream();
        _stopStreamButton.Click += (_, _) => NativeMethods.HostStopStream();
        _timer.Tick += (_, _) => PollEvents();

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
            _statusText.Text = "Abra a sala para começar.";
            _coreOk = true;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            _versionText.Text = "Motor Rust indisponível";
            _statusText.Text = "Problema com a lanshare_ffi.dll. Rode: cargo build --release -p lanshare-ffi";
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
            _statusText.Text = "Sala fechada.";
            UpdateUi();
            return;
        }

        var (ok, error) = NativeMethods.HostStart(_passwordBox.Text, null, _roomNameBox.Text);
        if (!ok)
        {
            _statusText.Text = error ?? "Erro ao abrir a sala.";
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
        if (!ok) _statusText.Text = error ?? "Erro ao iniciar a transmissão.";
    }

    /// Lê os eventos que o Rust acumulou e atualiza a tela.
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
                        _statusText.Text = "Sala aberta. Aguardando usuários.";
                        break;
                    case "user_joined":
                    case "user_left":
                    case "user_kicked":
                        changedUsers = true;
                        break;
                    case "stream_started":
                        _streaming = true;
                        _statusText.Text =
                            $"Transmitindo {e.GetProperty("width").GetInt32()}x{e.GetProperty("height").GetInt32()} " +
                            $"@ {e.GetProperty("fps").GetInt32()} FPS";
                        UpdateUi();
                        break;
                    case "stream_stats":
                        _statusText.Text =
                            $"Transmitindo — {e.GetProperty("mbps").GetDouble():F1} Mbps, " +
                            $"{e.GetProperty("clients").GetInt32()} usuário(s)";
                        break;
                    case "stream_stopped":
                        _streaming = false;
                        _statusText.Text = e.TryGetProperty("error", out var err) && err.ValueKind == JsonValueKind.String
                            ? "Transmissão interrompida: " + err.GetString()
                            : "Transmissão parada.";
                        UpdateUi();
                        break;
                }
            }
        }

        if (changedUsers) RefreshUsers();
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