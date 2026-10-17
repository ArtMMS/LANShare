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
    /// Client que entra no meio da transmissão não ouve o som de "transmissão iniciada".
    private static readonly TimeSpan StreamSoundGrace = TimeSpan.FromSeconds(3);

    private enum Mode { Home, Host, Client }
    private enum ClientState { Idle, Connecting, Connected }

    private sealed record RoomEntry(
        string Name, string Address, int Users, int? MaxUsers, bool HasPassword, bool Compatible);

    // Fonte dos ícones (Windows 11 / Windows 10)
    private static readonly Avalonia.Media.FontFamily IconFont =
        new("Segoe Fluent Icons, Segoe MDL2 Assets");

    // Estrutura geral
    private readonly Grid _mainLayout, _clientLobby;
    private readonly StackPanel _homePanel;
    private readonly DockPanel _hostPanel, _clientPanel;
    private readonly Border _clientConnecting;
    private readonly TextBlock _versionText, _placeholderText, _infoText;
    private readonly Image _previewImage;

    // Tela de nome (primeira abertura)
    private readonly Border _nameScreen;
    private readonly TextBlock _nameError;
    private readonly TextBox _nameInput;
    private readonly Button _nameConfirmButton;

    // Cartão de perfil (rodapé da lateral)
    private readonly TextBox _profileNameBox;
    private readonly TextBlock _profileNameError;

    // Início
    private readonly Button _createRoomButton, _joinRoomButton;

    // Host
    private readonly TextBlock _statusLabel;
    private readonly TextBox _roomNameBox, _passwordBox;
    private readonly Button _roomButton, _startStreamButton, _stopStreamButton, _hostBackButton;
    private readonly ComboBox _resolutionBox, _fpsBox;
    private readonly StackPanel _usersPanel;

    // Client
    private readonly TextBlock _clientStatusLabel, _roomsStatusText, _passwordHint,
        _clientUsersHeader, _connectingTitle, _connectingText;
    private readonly TextBox _clientAddressBox, _clientPasswordBox;
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
    private bool _wasStreaming; // para avisar só quando a transmissão começa

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
    private double? _syncMs; // diferença média entre imagem e som (null = sem som)
    private string? _videoSize;
    private DateTime _connectedAt;

    public MainWindow()
    {
        InitializeComponent();

        // Permite à janela receber o foco (usado para tirar o cursor do campo do nome)
        Focusable = true;

        _mainLayout = Find<Grid>("MainLayout");
        _clientLobby = Find<Grid>("ClientLobby");
        _homePanel = Find<StackPanel>("HomePanel");
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

        _profileNameBox = Find<TextBox>("ProfileNameBox");
        _profileNameError = Find<TextBlock>("ProfileNameError");

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
        _roomsStatusText = Find<TextBlock>("RoomsStatusText");
        _passwordHint = Find<TextBlock>("PasswordHint");
        _clientUsersHeader = Find<TextBlock>("ClientUsersHeader");
        _connectingTitle = Find<TextBlock>("ConnectingTitle");
        _connectingText = Find<TextBlock>("ConnectingText");
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

        // Painel de escolha do que transmitir, cartão de perfil (avatar) e avisos
        InitSourcePicker();
        InitAvatar();
        InitNotifications();
        InitVolume(); // antes do InitFullscreen: o Esc fecha o painel de volume antes de sair da tela cheia

        // Tela de nome (primeira abertura)
        _nameConfirmButton.Click += (_, _) => ConfirmName();
        _nameInput.KeyDown += (_, e) =>
        {
            if (e.Key != Key.Enter) return;
            ConfirmName();
            e.Handled = true;
        };

        // Nome editável no cartão de perfil
        WireInlineName(_profileNameBox, _profileNameError);

        // Início
        _createRoomButton.Click += (_, _) => { ShowMode(Mode.Host); UpdateUi(); };
        _joinRoomButton.Click += (_, _) => EnterClientMode();

        // Host ("Iniciar transmissão" vira "Alterar transmissão" enquanto transmite)
        _roomButton.Click += (_, _) => ToggleRoom();
        _startStreamButton.Click += (_, _) => OpenSourcePicker();
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

        InitFullscreen();

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

        // Procura versão nova no GitHub (só se estiver ligado nas configurações)
        _ = CheckForUpdatesOnStartupAsync();

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
        _versionText.Text = AppInfo.DisplayVersion;
        try
        {
            _ = NativeMethods.GetVersion(); // só confere que a DLL do Rust carregou
            NativeMethods.SetLanguage(Loc.Code); // mensagens do motor no idioma do app
            var saved = NativeMethods.GetUsername();
            _coreOk = true;
            return saved;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            _versionText.Text = Loc.F("Core_Unavailable", AppInfo.DisplayVersion);
            _placeholderText.Text = Loc.T("Core_DllProblem");
            _createRoomButton.IsEnabled = _joinRoomButton.IsEnabled = _profileNameBox.IsEnabled = false;
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
            SetInlineError(_nameError, error ?? Loc.T("Name_Invalid"));
            return;
        }

        ApplyUsername(name!);
        _nameScreen.IsVisible = false;
        _mainLayout.IsEnabled = true;
    }

    private void ApplyUsername(string name)
    {
        _username = name;
        _profileNameBox.Text = name;
    }

    /// O nome é enviado ao abrir a sala ou ao conectar: só dá para editar fora disso.
    private void UpdateProfileState()
    {
        _profileNameBox.IsEnabled = _coreOk && !_roomOpen && _clientState == ClientState.Idle;
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
            SetInlineError(error, message ?? Loc.T("Name_Invalid"));
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
            Mode.Host => Loc.T("Host_OpenToStart"),
            Mode.Client => Loc.T("Client_ChooseRoom"),
            _ => Loc.T("Home_ChooseOption"),
        });
        _infoText.Text = "";
        UpdateProfileState();
        UpdateVolumeControls();
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
            _infoText.Text = Loc.F("Common_Error", ex.Message);
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
            _peerAvatars.Clear();
            StopTimerIfIdle();
            _usersPanel.Children.Clear();
            UpdateUi();
            ShowMode(Mode.Home); // fechar a sala já volta para o início
            return;
        }

        // Se o nome ainda está sendo editado, salva antes (o motor lê o nome ao abrir a sala)
        CommitName(_profileNameBox, _profileNameError);

        var (ok, error) = NativeMethods.HostStart(_passwordBox.Text, null, _roomNameBox.Text);
        if (!ok)
        {
            _placeholderText.Text = error ?? Loc.T("Host_OpenError");
            return;
        }

        _roomOpen = true;
        _peerAvatars.Clear();
        NativeMethods.HostSetAvatar(AvatarStore.ExportForNetwork()); // vai para quem entrar
        _timer.Start();
        UpdateUi();
    }

    /// Começa a transmitir o monitor escolhido no painel ("index" da lista de monitores).
    private void StartStream(int monitorIndex)
    {
        var height = _resolutionBox.SelectedIndex == 0 ? 720u : 1080u;
        var fps = _fpsBox.SelectedIndex == 0 ? 15u : 30u;
        var (ok, error) = NativeMethods.HostStartStreamOn(height, fps, monitorIndex);
        if (!ok)
        {
            _infoText.Text = error ?? Loc.T("Stream_StartError");
            return;
        }

        _streaming = true;
        _infoText.Text = Loc.T("Stream_CapturingScreen");
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
                        _placeholderText.Text = Loc.T("Host_RoomOpenHint");
                        changedUsers = true; // mostra o próprio Host na lista
                        break;
                    case "user_joined":
                        Notify(Loc.F("Notify_Joined", EventName(e)), JoinGlyph, Sound.UserJoined);
                        changedUsers = true;
                        break;
                    case "user_left":
                        _peerAvatars.Remove(e.GetProperty("id").GetUInt64());
                        Notify(Loc.F("Notify_Left", EventName(e)), LeaveGlyph, Sound.UserLeft);
                        changedUsers = true;
                        break;
                    case "user_kicked":
                        _peerAvatars.Remove(e.GetProperty("id").GetUInt64());
                        Notify(Loc.F("Notify_Kicked", EventName(e)), KickGlyph, Sound.UserLeft);
                        changedUsers = true;
                        break;
                    case "user_avatar":
                        SetPeerAvatar(e.GetProperty("id").GetUInt64(), e.GetProperty("png_base64").GetString());
                        changedUsers = true;
                        break;
                    case "stream_started":
                        _infoText.Text = Loc.F(
                            "Stream_Started",
                            e.GetProperty("width").GetInt32(),
                            e.GetProperty("height").GetInt32(),
                            e.GetProperty("fps").GetInt32());
                        break;
                    case "stream_stats":
                        _infoText.Text = Loc.F(
                            "Stream_Stats",
                            e.GetProperty("mbps").GetDouble(),
                            e.GetProperty("clients").GetInt32());
                        break;
                    case "stream_stopped":
                        _streaming = false;
                        _infoText.Text = e.TryGetProperty("error", out var err) && err.ValueKind == JsonValueKind.String
                            ? Loc.F("Stream_Interrupted", err.GetString())
                            : "";
                        HidePreview(Loc.T("Stream_Stopped"));
                        UpdateUi();
                        break;
                }
            }
        }

        if (changedUsers) RefreshUsers();
    }

    /// Nome que veio num evento (ou "Alguém", se faltar).
    private static string EventName(JsonElement e) =>
        e.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String && !string.IsNullOrWhiteSpace(n.GetString())
            ? n.GetString()!
            : Loc.T("Common_Someone");

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

        _usersPanel.Children.Add(UserRow(_myAvatar, Loc.F("Users_YouHost", _username)));

        using var users = NativeMethods.HostUsers();
        foreach (var user in users.RootElement.EnumerateArray())
        {
            var id = user.GetProperty("id").GetUInt64();
            var name = user.GetProperty("name").GetString() ?? "?";
            var kick = new Button { Content = Loc.T("Users_Kick") };
            kick.Click += (_, _) => NativeMethods.HostKick(id);

            _usersPanel.Children.Add(UserRow(PeerAvatar(id), name, kick));
        }
    }

    private void UpdateUi()
    {
        // Aviso + som só no momento em que a transmissão começa (trocar de fonte não conta)
        if (_streaming && !_wasStreaming)
            Notify(Loc.T("Notify_YouStarted"), StreamGlyph, Sound.StreamStarted);
        _wasStreaming = _streaming;

        _statusLabel.Text = _roomOpen
            ? Loc.T(_streaming ? "Host_Streaming" : "Host_RoomOpen")
            : Loc.T("Host_RoomClosed");
        _roomButton.Content = Loc.T(_roomOpen ? "Host_CloseRoom" : "Host_OpenRoom");
        _roomNameBox.IsEnabled = _passwordBox.IsEnabled = !_roomOpen;

        // Transmitindo, o mesmo botão troca o monitor/janela sem parar a transmissão
        _startStreamButton.IsEnabled = _roomOpen;
        _startStreamButton.Content = Loc.T(_streaming ? "Stream_Change" : "Stream_Start");
        _stopStreamButton.IsEnabled = _roomOpen && _streaming;

        _hostBackButton.IsVisible = !_roomOpen; // com a sala aberta, "Fechar sala" já volta ao início

        if (_pickerOpen) UpdatePickerConfirm();
        UpdateProfileState();
    }

    // ───────────────────────── Client ─────────────────────────

    private void EnterClientMode()
    {
        var (ok, error) = NativeMethods.ClientStart();
        if (!ok)
        {
            HidePreview(error ?? Loc.T("Client_StartError"));
            return;
        }

        _clientActive = true;
        NativeMethods.ClientSetAvatar(AvatarStore.ExportForNetwork()); // vai ao Host ao entrar numa sala
        ApplyAudioOutput(); // por onde sai o som das transmissões
        ApplyVolume(); // volume salvo da última vez
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
        _peerAvatars.Clear();
        _passwordPanel.IsVisible = false;
        _clientPasswordBox.Text = "";
        _hostName = "Host";
        _kickedReason = null;
        _pingMs = -1;
        _videoFps = _videoMbps = 0;
        _videoSize = null;
        _syncMs = null;
    }

    private void ScanRooms()
    {
        if (!_clientActive) return;
        if (_roomCount == 0) SetRoomsStatus(Loc.T("Client_Searching"));
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
            _infoText.Text = Loc.T("Client_TypeIp");
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
            _infoText.Text = Loc.T("Client_IncompatibleRoom");
            return;
        }
        StartConnection(room.Address);
    }

    private void StartConnection(string address)
    {
        // Se o nome ainda está sendo editado, salva antes (o motor lê o nome do settings.json ao conectar)
        CommitName(_profileNameBox, _profileNameError);

        _kickedReason = null;
        var (ok, error) = NativeMethods.ClientConnect(address);
        if (!ok)
        {
            _infoText.Text = error ?? Loc.T("Client_ConnectError");
            return;
        }

        _clientState = ClientState.Connecting;
        _infoText.Text = "";
        _connectingTitle.Text = Loc.T("Client_Connecting");
        _connectingText.Text = Loc.F("Client_ConnectingTo", address);
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
            _infoText.Text = error ?? Loc.T("Client_PasswordSendError");
            return;
        }

        _clientPasswordBox.Text = "";
        _passwordPanel.IsVisible = false;
        _connectingTitle.Text = Loc.T("Client_Connecting");
        _connectingText.Text = Loc.T("Client_CheckingPassword");
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
                        SetRoomsStatus(e.GetProperty("error").GetString() ?? Loc.T("Client_ScanError"));
                        break;

                    case "connecting":
                        _connectingText.Text = Loc.F("Client_ConnectingTo", e.GetProperty("address").GetString());
                        break;

                    case "password_required":
                        _connectingTitle.Text = Loc.T("Client_ProtectedRoom");
                        _connectingText.Text = "";
                        _passwordHint.Text = e.GetProperty("retry").GetBoolean()
                            ? Loc.T("Client_WrongPassword")
                            : Loc.T("Client_RoomHasPassword");
                        _clientPasswordBox.Text = "";
                        _passwordPanel.IsVisible = true;
                        _clientPasswordBox.Focus();
                        break;

                    case "connected":
                        _clientState = ClientState.Connected;
                        _connectedAt = DateTime.UtcNow;
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
                        HidePreview(Loc.T("Client_WaitingStream"));
                        RefreshClientUsers();
                        UpdateClientUi();
                        RefreshClientInfo();
                        break;

                    case "user_joined":
                        _clientUsers[e.GetProperty("id").GetUInt64()] = e.GetProperty("name").GetString() ?? "?";
                        Notify(Loc.F("Notify_Joined", EventName(e)), JoinGlyph, Sound.UserJoined);
                        RefreshClientUsers();
                        break;

                    case "user_left":
                    case "user_kicked":
                    {
                        var id = e.GetProperty("id").GetUInt64();
                        var kickedOut = e.GetProperty("type").GetString() == "user_kicked";
                        _clientUsers.Remove(id);
                        _peerAvatars.Remove(id);
                        Notify(
                            Loc.F(kickedOut ? "Notify_Kicked" : "Notify_Left", EventName(e)),
                            kickedOut ? KickGlyph : LeaveGlyph,
                            Sound.UserLeft);
                        RefreshClientUsers();
                        break;
                    }

                    case "user_avatar":
                        SetPeerAvatar(e.GetProperty("id").GetUInt64(), e.GetProperty("png_base64").GetString());
                        RefreshClientUsers();
                        break;

                    case "kicked":
                        _kickedReason = e.GetProperty("reason").GetString() ?? Loc.T("Client_KickedDefault");
                        _infoText.Text = _kickedReason;
                        break;

                    case "ping":
                        _pingMs = (int)e.GetProperty("rtt_ms").GetUInt64();
                        RefreshClientInfo();
                        break;

                    case "video_started":
                    {
                        // Primeira imagem de uma transmissão nova (não de uma troca de monitor/janela)
                        var newStream = _videoSize is null;
                        _videoSize = $"{e.GetProperty("width").GetInt32()}x{e.GetProperty("height").GetInt32()}";
                        if (newStream && DateTime.UtcNow - _connectedAt > StreamSoundGrace)
                            Notify(Loc.F("Notify_HostStarted", _hostName), StreamGlyph, Sound.StreamStarted);
                        RefreshClientInfo();
                        break;
                    }

                    case "sync_stats":
                        _syncMs = e.TryGetProperty("offset_ms", out var off) && off.ValueKind == JsonValueKind.Number
                            ? off.GetDouble()
                            : null;
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
                        _syncMs = null;
                        HidePreview(Loc.T("Stream_Stopped"));
                        RefreshClientInfo();
                        break;

                    case "audio_error":
                        // O vídeo continua; só avisa (uma vez por problema) que o som não está saindo
                        Notify(e.GetProperty("error").GetString() ?? Loc.T("Client_AudioError"), AudioErrorGlyph);
                        break;

                    case "video_error":
                        _infoText.Text = e.GetProperty("error").GetString() ?? Loc.T("Client_VideoError");
                        break;

                    case "rejected":
                        EndClientSession(Loc.F("Client_Rejected", e.GetProperty("reason").GetString()));
                        break;

                    case "connect_failed":
                        EndClientSession(e.GetProperty("error").GetString() ?? Loc.T("Client_ConnectFailed"));
                        break;

                    case "disconnected":
                        var local = e.TryGetProperty("local", out var l) && l.ValueKind == JsonValueKind.True;
                        var message = _kickedReason
                            ?? (local ? "" : Loc.F("Client_DisconnectedReason", e.GetProperty("reason").GetString()));
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
                r.GetProperty("name").GetString() ?? Loc.T("Room_DefaultName"),
                r.GetProperty("address").GetString() ?? "",
                r.GetProperty("users").GetInt32(),
                r.TryGetProperty("max_users", out var max) && max.ValueKind == JsonValueKind.Number
                    ? max.GetInt32()
                    : (int?)null,
                r.GetProperty("has_password").GetBoolean(),
                r.GetProperty("compatible").GetBoolean()));
        }

        _roomCount = entries.Count;
        SetRoomsStatus(entries.Count == 0 ? Loc.T("Client_NoRooms") : "");

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
        var people = room.MaxUsers is { } max
            ? Loc.F("Room_UsersMax", room.Users, max)
            : Loc.F("Room_Users", room.Users);

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
        if (room.HasPassword) details.Children.Add(InfoLine("\uE72E", Loc.T("Room_WithPassword")));
        if (!room.Compatible) details.Children.Add(InfoLine("\uE7BA", Loc.T("Room_Incompatible")));

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

    /// Lista da sala: primeiro o Host, depois os clients (você marcado), cada um com o avatar.
    private void RefreshClientUsers()
    {
        _clientUsersPanel.Children.Clear();
        _clientUsersPanel.Children.Add(UserRow(PeerAvatar(HostUserId), Loc.F("Users_Host", _hostName)));
        foreach (var (id, name) in _clientUsers.OrderBy(u => u.Key))
        {
            var mine = id == _myId;
            _clientUsersPanel.Children.Add(UserRow(mine ? _myAvatar : PeerAvatar(id), mine ? Loc.F("Users_You", name) : name));
        }
    }

    /// Linha de informações embaixo do vídeo: ping, resolução, FPS e Mbps.
    private void RefreshClientInfo()
    {
        if (_clientState != ClientState.Connected || _kickedReason is not null) return;

        var parts = new List<string> { Loc.T("Client_Connected") };
        if (_pingMs >= 0) parts.Add($"ping {_pingMs} ms");
        if (_videoSize is not null) parts.Add($"{_videoSize} @ {_videoFps:F0} FPS, {_videoMbps:F1} Mbps");
        if (_videoSize is not null && _syncMs is { } sync) parts.Add($"A/V {sync:+0;-0;0} ms");
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

        _clientStatusLabel.Text = _clientState switch
        {
            ClientState.Idle => Loc.T("Client_Disconnected"),
            ClientState.Connecting => Loc.T("Client_Connecting"),
            _ => Loc.T("Client_Connected"),
        };

        _clientUsersHeader.IsVisible = _clientUsersPanel.IsVisible = connected;
        _clientDisconnectButton.IsVisible = connected;
        _clientBackButton.Content = Loc.T(idle ? "Common_BackHome" : "Client_LeaveAndBack");

        UpdateProfileState();
        UpdateStage();
        UpdateVolumeControls(); // o botão de volume só existe com o Client conectado
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
