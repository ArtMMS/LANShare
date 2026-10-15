using System.Runtime.InteropServices;
using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Markup.Xaml;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;

namespace LANShare.App;

public partial class MainWindow : Window
{
    /// Versão mostrada ao lado do nome do app.
    private const string AppVersion = "v1.0";

    private enum Mode { Home, Host, Client }
    private enum ClientState { Idle, Connecting, Connected }

    private sealed record RoomEntry(
        string Name, string Address, int Users, int? MaxUsers, bool HasPassword, bool Compatible);

    // Fonte dos ícones (Windows 11 / Windows 10)
    private static readonly Avalonia.Media.FontFamily IconFont =
        new("Segoe Fluent Icons, Segoe MDL2 Assets");

    // Estrutura geral
    private readonly Grid _mainLayout, _clientLobby;
    private readonly DockPanel _homePanel, _hostPanel, _clientPanel;
    private readonly Border _clientConnecting;
    private readonly TextBlock _versionText, _placeholderText, _infoText;
    private readonly Image _previewImage;

    // Tela de nome (primeira abertura)
    private readonly Border _nameScreen;
    private readonly TextBlock _nameError;
    private readonly TextBox _nameInput;
    private readonly Button _nameConfirmButton;

    // Início
    private readonly TextBox _homeNameBox;
    private readonly TextBlock _homeNameError;
    private readonly Button _createRoomButton, _joinRoomButton;

    // Host
    private readonly TextBlock _statusLabel;
    private readonly TextBox _roomNameBox, _passwordBox;
    private readonly Button _roomButton, _startStreamButton, _stopStreamButton, _hostBackButton;
    private readonly ComboBox _resolutionBox, _fpsBox;
    private readonly StackPanel _usersPanel;

    // Client
    private readonly TextBlock _clientStatusLabel, _clientNameError, _roomsStatusText, _passwordHint,
        _clientUsersHeader, _connectingTitle, _connectingText;
    private readonly TextBox _clientNameBox, _clientAddressBox, _clientPasswordBox;
    private readonly Button _refreshRoomsButton, _passwordConfirmButton, _clientConnectButton,
        _clientBackButton, _clientDisconnectButton, _connectingCancelButton;
    private readonly ListBox _roomsList;
    private readonly StackPanel _passwordPanel, _clientUsersPanel;

    private readonly DispatcherTimer _timer = new() { Interval = TimeSpan.FromMilliseconds(16) };
    private readonly DispatcherTimer _scanTimer = new() { Interval = TimeSpan.FromSeconds(5) };

    // Buffer reaproveitado para receber a imagem do Rust (preview do Host ou tela do Client)
    private readonly byte[] _previewBuffer = new byte[NativeMethods.PreviewBufferSize];
    private WriteableBitmap? _previewBitmap;

    private Mode _mode = Mode.Home;
    private bool _coreOk;
    private string? _username;

    // Host
    private bool _roomOpen;
    private bool _streaming;

    // Client
    private bool _clientActive;
    private ClientState _clientState = ClientState.Idle;
    private readonly Dictionary<ulong, string> _clientUsers = new();
    private ulong _myId;
    private string _hostName = "Host";
    private string? _roomsSignature;
    private int _roomCount;
    private string? _kickedReason;
    private int _pingMs = -1;
    private double _videoFps, _videoMbps;
    private string? _videoSize;

    public MainWindow()
    {
        InitializeComponent();

        // Permite à janela receber o foco (usado para tirar o cursor do campo do nome)
        Focusable = true;

        _mainLayout = Find<Grid>("MainLayout");
        _clientLobby = Find<Grid>("ClientLobby");
        _homePanel = Find<DockPanel>("HomePanel");
        _hostPanel = Find<DockPanel>("HostPanel");
        _clientPanel = Find<DockPanel>("ClientPanel");
        _clientConnecting = Find<Border>("ClientConnecting");
        _versionText = Find<TextBlock>("VersionText");
        _placeholderText = Find<TextBlock>("PlaceholderText");
        _infoText = Find<TextBlock>("InfoText");
        _previewImage = Find<Image>("PreviewImage");

        _nameScreen = Find<Border>("NameScreen");
        _nameError = Find<TextBlock>("NameError");
        _nameInput = Find<TextBox>("NameInput");
        _nameConfirmButton = Find<Button>("NameConfirmButton");

        _homeNameBox = Find<TextBox>("HomeNameBox");
        _homeNameError = Find<TextBlock>("HomeNameError");
        _createRoomButton = Find<Button>("CreateRoomButton");
        _joinRoomButton = Find<Button>("JoinRoomButton");

        _statusLabel = Find<TextBlock>("StatusLabel");
        _roomNameBox = Find<TextBox>("RoomNameBox");
        _passwordBox = Find<TextBox>("PasswordBox");
        _roomButton = Find<Button>("RoomButton");
        _startStreamButton = Find<Button>("StartStreamButton");
        _stopStreamButton = Find<Button>("StopStreamButton");
        _hostBackButton = Find<Button>("HostBackButton");
        _resolutionBox = Find<ComboBox>("ResolutionBox");
        _fpsBox = Find<ComboBox>("FpsBox");
        _usersPanel = Find<StackPanel>("UsersPanel");

        _clientStatusLabel = Find<TextBlock>("ClientStatusLabel");
        _clientNameError = Find<TextBlock>("ClientNameError");
        _roomsStatusText = Find<TextBlock>("RoomsStatusText");
        _passwordHint = Find<TextBlock>("PasswordHint");
        _clientUsersHeader = Find<TextBlock>("ClientUsersHeader");
        _connectingTitle = Find<TextBlock>("ConnectingTitle");
        _connectingText = Find<TextBlock>("ConnectingText");
        _clientNameBox = Find<TextBox>("ClientNameBox");
        _clientAddressBox = Find<TextBox>("ClientAddressBox");
        _clientPasswordBox = Find<TextBox>("ClientPasswordBox");
        _refreshRoomsButton = Find<Button>("RefreshRoomsButton");
        _passwordConfirmButton = Find<Button>("PasswordConfirmButton");
        _clientConnectButton = Find<Button>("ClientConnectButton");
        _clientBackButton = Find<Button>("ClientBackButton");
        _clientDisconnectButton = Find<Button>("ClientDisconnectButton");
        _connectingCancelButton = Find<Button>("ConnectingCancelButton");
        _roomsList = Find<ListBox>("RoomsList");
        _passwordPanel = Find<StackPanel>("PasswordPanel");
        _clientUsersPanel = Find<StackPanel>("ClientUsersPanel");

        // Tela de nome (primeira abertura)
        _nameConfirmButton.Click += (_, _) => ConfirmName();
        _nameInput.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            ConfirmName();
            e.Handled = true;
        };

        // Nome editável (início e Client)
        WireInlineName(_homeNameBox, _homeNameError);
        WireInlineName(_clientNameBox, _clientNameError);

        // Início
        _createRoomButton.Click += (_, _) => { ShowMode(Mode.Host); UpdateUi(); };
        _joinRoomButton.Click += (_, _) => EnterClientMode();

        // Host
        _roomButton.Click += (_, _) => ToggleRoom();
        _startStreamButton.Click += (_, _) => StartStream();
        _stopStreamButton.Click += (_, _) => NativeMethods.HostStopStream();
        _hostBackButton.Click += (_, _) => ShowMode(Mode.Home); // só aparece com a sala fechada

        // Client
        _refreshRoomsButton.Click += (_, _) => ScanRooms();
        _roomsList.DoubleTapped += (_, _) => ConnectToSelectedRoom();
        _roomsList.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            ConnectToSelectedRoom();
            e.Handled = true;
        };
        _clientConnectButton.Click += (_, _) => OnClientConnectClick();
        _clientAddressBox.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            OnClientConnectClick();
            e.Handled = true;
        };
        _passwordConfirmButton.Click += (_, _) => SubmitPassword();
        _clientPasswordBox.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            SubmitPassword();
            e.Handled = true;
        };
        _connectingCancelButton.Click += (_, _) => NativeMethods.ClientDisconnect();
        _clientDisconnectButton.Click += (_, _) => NativeMethods.ClientDisconnect();
        _clientBackButton.Click += (_, _) => LeaveClientMode();

        _timer.Tick += (_, _) => OnTick();
        _scanTimer.Tick += (_, _) =>
        {
            if (_clientActive && _clientState == ClientState.Idle) ScanRooms();
        };

        var savedName = ShowCoreStatus();
        if (_coreOk)
        {
            ShowMode(Mode.Home);
            if (string.IsNullOrWhiteSpace(savedName))
            {
                OpenNameScreen();
            }
            else
            {
                ApplyUsername(savedName);
            }
        }

        Opened += (_, _) =>
        {
            if (_nameScreen.IsVisible) _nameInput.Focus();
        };
        Closing += (_, _) =>
        {
            if (!_coreOk) return;
            NativeMethods.HostStop();
            NativeMethods.ClientStop();
        };
    }

    private void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    private T Find<T>(string name) where T : Control => this.FindControl<T>(name)!;

    /// Confere se a ponte C# ↔ Rust funciona. Devolve o nome salvo (null na primeira abertura).
    private string? ShowCoreStatus()
    {
        _versionText.Text = AppVersion;
        try
        {
            _ = NativeMethods.GetVersion(); // só confere que a DLL do Rust carregou
            var saved = NativeMethods.GetUsername();
            _coreOk = true;
            return saved;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            _versionText.Text = $"{AppVersion} — motor Rust indisponível";
            _placeholderText.Text = "Problema com a lanshare_ffi.dll. Rode: cargo build --release -p lanshare-ffi";
            _createRoomButton.IsEnabled = _joinRoomButton.IsEnabled = _homeNameBox.IsEnabled = false;
            return null;
        }
    }

    // ───────────────────────── Nome de usuário ─────────────────────────

    /// Tela de primeira abertura (por cima de tudo, dentro do app).
    private void OpenNameScreen()
    {
        _nameInput.Text = "";
        SetInlineError(_nameError, null);

        _nameScreen.IsVisible = true;
        _mainLayout.IsEnabled = false;
        Dispatcher.UIThread.Post(() => _nameInput.Focus());
    }

    private void ConfirmName()
    {
        var (ok, name, error) = NativeMethods.SetUsername(_nameInput.Text);
        if (!ok)
        {
            SetInlineError(_nameError, error ?? "Nome inválido.");
            return;
        }

        ApplyUsername(name!);
        _nameScreen.IsVisible = false;
        _mainLayout.IsEnabled = true;
    }

    private void ApplyUsername(string name)
    {
        _username = name;
        _homeNameBox.Text = name;
        _clientNameBox.Text = name;
    }

    /// Nome que parece texto: clicar edita; Enter ou clicar fora salva; Esc cancela.
    private void WireInlineName(TextBox box, TextBlock error)
    {
        box.LostFocus += (_, _) => CommitName(box, error);
        box.KeyDown += (_, e) =>
        {
            if (e.Key == Key.Enter)
            {
                CommitName(box, error);
                Focus(); // tira o cursor do campo
                e.Handled = true;
            }
            else if (e.Key == Key.Escape)
            {
                box.Text = _username;
                SetInlineError(error, null);
                Focus();
                e.Handled = true;
            }
        };
    }

    private void CommitName(TextBox box, TextBlock error)
    {
        var typed = box.Text?.Trim() ?? "";
        if (string.Equals(typed, _username, StringComparison.Ordinal))
        {
            box.Text = _username;
            return;
        }

        var (ok, name, message) = NativeMethods.SetUsername(typed);
        if (!ok)
        {
            box.Text = _username; // volta ao nome anterior
            SetInlineError(error, message ?? "Nome inválido.");
            DispatcherTimer.RunOnce(() => SetInlineError(error, null), TimeSpan.FromSeconds(3));
            return;
        }

        ApplyUsername(name!);
        SetInlineError(error, null);
    }

    private static void SetInlineError(TextBlock label, string? message)
    {
        label.Text = message ?? "";
        label.IsVisible = !string.IsNullOrEmpty(message);
    }

    // ───────────────────────── Modos (Início / Host / Client) ─────────────────────────

    private void ShowMode(Mode mode)
    {
        _mode = mode;
        _homePanel.IsVisible = mode == Mode.Home;
        _hostPanel.IsVisible = mode == Mode.Host;
        _clientPanel.IsVisible = mode == Mode.Client;

        HidePreview(mode switch
        {
            Mode.Host => "Abra a sala para começar.",
            Mode.Client => "Escolha uma sala para entrar.",
            _ => "Escolha uma opção ao lado para começar.",
        });
        _infoText.Text = "";
    }

    /// Decide o que aparece na área grande: salas, conexão, imagem ou mensagem.
    private void UpdateStage()
    {
        var lobby = _mode == Mode.Client && _clientState == ClientState.Idle;
        var connecting = _mode == Mode.Client && _clientState == ClientState.Connecting;

        _clientLobby.IsVisible = lobby;
        _clientConnecting.IsVisible = connecting;
        _placeholderText.IsVisible = !lobby && !connecting && !_previewImage.IsVisible;
    }

    private void StopTimerIfIdle()
    {
        if (!_roomOpen && !_clientActive) _timer.Stop();
    }

    /// A cada ~16 ms (60x por segundo): lê os eventos do Rust e atualiza a imagem.
    private void OnTick()
    {
        try
        {
            if (_roomOpen)
            {
                PollHostEvents();
                if (_streaming) UpdatePreview();
            }

            if (_clientActive)
            {
                PollClientEvents();
                if (_clientState == ClientState.Connected) UpdateClientFrame();
            }
        }
        catch (Exception ex)
        {
            _infoText.Text = "Erro: " + ex.Message;
        }
    }

    // ───────────────────────── Host ─────────────────────────

    private void ToggleRoom()
    {
        if (_roomOpen)
        {
            NativeMethods.HostStop();
            _roomOpen = false;
            _streaming = false;
            StopTimerIfIdle();
            _usersPanel.Children.Clear();
            UpdateUi();
            ShowMode(Mode.Home); // fechar a sala já volta para o início
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

    private void PollHostEvents()
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
                        _placeholderText.Text = "Sala aberta. Clique em Iniciar transmissão.";
                        changedUsers = true; // mostra o próprio Host na lista
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
        PaintFrame(width, height);
    }

    /// Lista da sala: primeiro o próprio Host, depois os clients (com o botão Expulsar).
    private void RefreshUsers()
    {
        _usersPanel.Children.Clear();
        if (!_roomOpen) return;

        _usersPanel.Children.Add(new TextBlock { Text = $"{_username} (você, host)" });

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
            row.Children.Add(new TextBlock { Text = name, VerticalAlignment = VerticalAlignment.Center });
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
        _hostBackButton.IsVisible = !_roomOpen; // com a sala aberta, "Fechar sala" já volta ao início
    }

    // ───────────────────────── Client ─────────────────────────

    private void EnterClientMode()
    {
        var (ok, error) = NativeMethods.ClientStart();
        if (!ok)
        {
            HidePreview(error ?? "Erro ao iniciar o modo cliente.");
            return;
        }

        _clientActive = true;
        ResetClientSession();
        _roomsList.ItemsSource = null;
        _roomsSignature = null;
        _roomCount = 0;
        _clientAddressBox.Text = "";

        ShowMode(Mode.Client);
        UpdateClientUi();

        _timer.Start();
        _scanTimer.Start();
        ScanRooms();
    }

    private void LeaveClientMode()
    {
        NativeMethods.ClientStop(); // desconecta (manda o Bye) e encerra o motor
        _clientActive = false;
        _scanTimer.Stop();
        ResetClientSession();
        _roomsList.ItemsSource = null;
        _roomsSignature = null;
        _roomCount = 0;
        SetRoomsStatus("");
        StopTimerIfIdle();
        ShowMode(Mode.Home);
    }

    private void ResetClientSession()
    {
        _clientState = ClientState.Idle;
        _clientUsers.Clear();
        _clientUsersPanel.Children.Clear();
        _passwordPanel.IsVisible = false;
        _clientPasswordBox.Text = "";
        _hostName = "Host";
        _kickedReason = null;
        _pingMs = -1;
        _videoFps = _videoMbps = 0;
        _videoSize = null;
    }

    private void ScanRooms()
    {
        if (!_clientActive) return;
        if (_roomCount == 0) SetRoomsStatus("Procurando salas...");
        NativeMethods.ClientScan(); // se já estiver procurando, o Rust ignora
    }

    private void SetRoomsStatus(string message)
    {
        _roomsStatusText.Text = message;
        _roomsStatusText.IsVisible = !string.IsNullOrEmpty(message);
    }

    /// Botão "Conectar" do cartão de IP.
    private void OnClientConnectClick()
    {
        if (_clientState != ClientState.Idle) return;

        var typed = _clientAddressBox.Text?.Trim();
        if (string.IsNullOrEmpty(typed))
        {
            _infoText.Text = "Digite o IP da sala ou dê um duplo clique em uma sala da lista.";
            return;
        }

        StartConnection(typed);
    }

    private void ConnectToSelectedRoom()
    {
        if (_clientState != ClientState.Idle) return;
        if (_roomsList.SelectedItem is not ListBoxItem { Tag: RoomEntry room }) return;

        if (!room.Compatible)
        {
            _infoText.Text = "Esta sala usa uma versão incompatível do LANShare.";
            return;
        }
        StartConnection(room.Address);
    }

    private void StartConnection(string address)
    {
        // Se o nome ainda está sendo editado, salva antes (o motor lê o nome do settings.json ao conectar)
        CommitName(_clientNameBox, _clientNameError);

        _kickedReason = null;
        var (ok, error) = NativeMethods.ClientConnect(address);
        if (!ok)
        {
            _infoText.Text = error ?? "Erro ao conectar.";
            return;
        }

        _clientState = ClientState.Connecting;
        _infoText.Text = "";
        _connectingTitle.Text = "Conectando...";
        _connectingText.Text = $"Conectando a {address}...";
        _passwordPanel.IsVisible = false;
        UpdateClientUi();
    }

    private void SubmitPassword()
    {
        var password = _clientPasswordBox.Text ?? "";
        if (password.Length == 0) return;

        var (ok, error) = NativeMethods.ClientSubmitPassword(password);
        if (!ok)
        {
            _infoText.Text = error ?? "Erro ao enviar a senha.";
            return;
        }

        _clientPasswordBox.Text = "";
        _passwordPanel.IsVisible = false;
        _connectingTitle.Text = "Conectando...";
        _connectingText.Text = "Verificando a senha...";
    }

    private void PollClientEvents()
    {
        while (NativeMethods.ClientPollEvent() is { } doc)
        {
            using (doc)
            {
                var e = doc.RootElement;
                switch (e.GetProperty("type").GetString())
                {
                    case "rooms":
                        ApplyRooms(e.GetProperty("rooms"));
                        break;

                    case "scan_failed":
                        SetRoomsStatus(e.GetProperty("error").GetString() ?? "Erro ao procurar salas.");
                        break;

                    case "connecting":
                        _connectingText.Text = $"Conectando a {e.GetProperty("address").GetString()}...";
                        break;

                    case "password_required":
                        _connectingTitle.Text = "Sala protegida";
                        _connectingText.Text = "";
                        _passwordHint.Text = e.GetProperty("retry").GetBoolean()
                            ? "Senha incorreta. Tente novamente."
                            : "Esta sala tem senha.";
                        _clientPasswordBox.Text = "";
                        _passwordPanel.IsVisible = true;
                        _clientPasswordBox.Focus();
                        break;

                    case "connected":
                        _clientState = ClientState.Connected;
                        _myId = e.GetProperty("client_id").GetUInt64();
                        _hostName = e.TryGetProperty("host_name", out var hn)
                                    && hn.ValueKind == JsonValueKind.String
                                    && !string.IsNullOrWhiteSpace(hn.GetString())
                            ? hn.GetString()!
                            : "Host";
                        _clientUsers.Clear();
                        foreach (var u in e.GetProperty("users").EnumerateArray())
                            _clientUsers[u.GetProperty("id").GetUInt64()] = u.GetProperty("name").GetString() ?? "?";
                        _pingMs = -1;
                        _videoFps = _videoMbps = 0;
                        _videoSize = null;
                        _passwordPanel.IsVisible = false;
                        HidePreview("Conectado. Aguardando a transmissão do Host.");
                        RefreshClientUsers();
                        UpdateClientUi();
                        RefreshClientInfo();
                        break;

                    case "user_joined":
                        _clientUsers[e.GetProperty("id").GetUInt64()] = e.GetProperty("name").GetString() ?? "?";
                        RefreshClientUsers();
                        break;

                    case "user_left":
                    case "user_kicked":
                        _clientUsers.Remove(e.GetProperty("id").GetUInt64());
                        RefreshClientUsers();
                        break;

                    case "kicked":
                        _kickedReason = e.GetProperty("reason").GetString() ?? "Você foi removido pelo Host.";
                        _infoText.Text = _kickedReason;
                        break;

                    case "ping":
                        _pingMs = (int)e.GetProperty("rtt_ms").GetUInt64();
                        RefreshClientInfo();
                        break;

                    case "video_started":
                        _videoSize = $"{e.GetProperty("width").GetInt32()}x{e.GetProperty("height").GetInt32()}";
                        RefreshClientInfo();
                        break;

                    case "video_stats":
                        _videoFps = e.GetProperty("fps").GetDouble();
                        _videoMbps = e.GetProperty("mbps").GetDouble();
                        RefreshClientInfo();
                        break;

                    case "stream_stopped":
                        // O Host parou de transmitir: a conexão continua, só a imagem acaba
                        _videoFps = _videoMbps = 0;
                        _videoSize = null;
                        HidePreview("Transmissão parada.");
                        RefreshClientInfo();
                        break;

                    case "video_error":
                        _infoText.Text = e.GetProperty("error").GetString() ?? "Erro no vídeo.";
                        break;

                    case "rejected":
                        EndClientSession("Entrada recusada: " + e.GetProperty("reason").GetString());
                        break;

                    case "connect_failed":
                        EndClientSession(e.GetProperty("error").GetString() ?? "Não foi possível conectar.");
                        break;

                    case "disconnected":
                        var local = e.TryGetProperty("local", out var l) && l.ValueKind == JsonValueKind.True;
                        var message = _kickedReason
                            ?? (local ? "" : "Desconectado: " + e.GetProperty("reason").GetString());
                        EndClientSession(message);
                        break;
                }
            }
        }
    }

    /// Volta o Client para "desconectado" (a lista de salas volta para a tela).
    private void EndClientSession(string message)
    {
        ResetClientSession();
        HidePreview("");
        _infoText.Text = message;
        UpdateClientUi();
        ScanRooms();
    }

    private void ApplyRooms(JsonElement rooms)
    {
        var entries = new List<RoomEntry>();
        foreach (var r in rooms.EnumerateArray())
        {
            entries.Add(new RoomEntry(
                r.GetProperty("name").GetString() ?? "Sala",
                r.GetProperty("address").GetString() ?? "",
                r.GetProperty("users").GetInt32(),
                r.TryGetProperty("max_users", out var max) && max.ValueKind == JsonValueKind.Number
                    ? max.GetInt32()
                    : (int?)null,
                r.GetProperty("has_password").GetBoolean(),
                r.GetProperty("compatible").GetBoolean()));
        }

        _roomCount = entries.Count;
        SetRoomsStatus(entries.Count == 0 ? "Nenhuma sala encontrada na rede." : "");

        // Só refaz a lista se algo mudou (para não perder a seleção a cada busca)
        var signature = string.Join("|", entries.Select(x =>
            $"{x.Name};{x.Address};{x.Users};{x.MaxUsers};{x.HasPassword};{x.Compatible}"));
        if (signature == _roomsSignature) return;
        _roomsSignature = signature;

        var selected = (_roomsList.SelectedItem as ListBoxItem)?.Tag as RoomEntry;
        var items = entries.Select(BuildRoomItem).ToList();
        _roomsList.ItemsSource = items;
        if (selected is not null)
            _roomsList.SelectedItem = items.FirstOrDefault(i => (i.Tag as RoomEntry)?.Address == selected.Address);
    }

    private static TextBlock GlyphIcon(string glyph, double size) => new()
    {
        Text = glyph,
        FontFamily = IconFont,
        FontSize = size,
        Opacity = 0.8,
        VerticalAlignment = VerticalAlignment.Center,
    };

    private static StackPanel InfoLine(string glyph, string text)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        row.Children.Add(GlyphIcon(glyph, 12));
        row.Children.Add(new TextBlock { Text = text, FontSize = 12, Opacity = 0.75 });
        return row;
    }

    /// Cartão de uma sala: nome, endereço, usuários e senha.
    private static ListBoxItem BuildRoomItem(RoomEntry room)
    {
        var people = room.MaxUsers is { } max ? $"{room.Users}/{max} na sala" : $"{room.Users} na sala";

        var details = new StackPanel { Spacing = 4 };
        details.Children.Add(new TextBlock
        {
            Text = room.Name,
            FontSize = 18,
            FontWeight = Avalonia.Media.FontWeight.SemiBold,
            TextTrimming = Avalonia.Media.TextTrimming.CharacterEllipsis,
        });
        details.Children.Add(new TextBlock { Text = room.Address, FontSize = 12, Opacity = 0.7 });
        details.Children.Add(InfoLine("\uE716", people));
        if (room.HasPassword) details.Children.Add(InfoLine("\uE72E", "com senha"));
        if (!room.Compatible) details.Children.Add(InfoLine("\uE7BA", "versão incompatível"));

        var grid = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto") };
        grid.Children.Add(details);
        if (room.HasPassword)
        {
            var lockIcon = GlyphIcon("\uE72E", 16);
            lockIcon.VerticalAlignment = VerticalAlignment.Top;
            Grid.SetColumn(lockIcon, 1);
            grid.Children.Add(lockIcon);
        }

        var item = new ListBoxItem { Content = grid, Tag = room };
        item.Classes.Add("room");
        return item;
    }

    /// Lista da sala: primeiro o Host, depois os clients (você marcado).
    private void RefreshClientUsers()
    {
        _clientUsersPanel.Children.Clear();
        _clientUsersPanel.Children.Add(new TextBlock { Text = $"{_hostName} (host)" });
        foreach (var (id, name) in _clientUsers.OrderBy(u => u.Key))
        {
            _clientUsersPanel.Children.Add(new TextBlock { Text = id == _myId ? $"{name} (você)" : name });
        }
    }

    /// Linha de informações embaixo do vídeo: ping, resolução, FPS e Mbps.
    private void RefreshClientInfo()
    {
        if (_clientState != ClientState.Connected || _kickedReason is not null) return;

        var parts = new List<string> { "Conectado" };
        if (_pingMs >= 0) parts.Add($"ping {_pingMs} ms");
        if (_videoSize is not null) parts.Add($"{_videoSize} @ {_videoFps:F0} FPS, {_videoMbps:F1} Mbps");
        _infoText.Text = string.Join(" — ", parts);
    }

    private void UpdateClientFrame()
    {
        if (!NativeMethods.ClientFrame(_previewBuffer, out var width, out var height)) return;
        PaintFrame(width, height);
    }

    private void UpdateClientUi()
    {
        var idle = _clientState == ClientState.Idle;
        var connected = _clientState == ClientState.Connected;

        _clientNameBox.IsEnabled = idle; // o nome é enviado ao conectar
        _clientStatusLabel.Text = _clientState switch
        {
            ClientState.Idle => "Desconectado",
            ClientState.Connecting => "Conectando...",
            _ => "Conectado",
        };

        _clientUsersHeader.IsVisible = _clientUsersPanel.IsVisible = connected;
        _clientDisconnectButton.IsVisible = connected;
        _clientBackButton.Content = idle ? "Voltar ao início" : "Sair da sala e voltar";

        UpdateStage();
    }

    // ───────────────────────── Imagem (Host e Client) ─────────────────────────

    /// Desenha na área grande a imagem BGRA que está em _previewBuffer.
    private void PaintFrame(int width, int height)
    {
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

        if (!_previewImage.IsVisible)
        {
            _previewImage.IsVisible = true;
            UpdateStage();
        }
    }

    private void HidePreview(string message)
    {
        _previewImage.IsVisible = false;
        _previewImage.Source = null;
        _previewBitmap = null;
        _placeholderText.Text = message;
        UpdateStage();
    }
}