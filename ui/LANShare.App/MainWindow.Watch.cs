using System.Text.Json;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;

namespace LANShare.App;

/// Assistir (v2.0), no Host e no Client:
/// - quem transmite ganha o ícone de tela ao lado do nome (cinza; verde quando eu assisto);
///   clicar assiste (clicar de novo para);
/// - quem você assiste fica com a borda verde na lista de usuários;
/// - a transmissão escolhida aparece na área grande, com volume e tela cheia;
/// - "Fulano iniciou/encerrou a transmissão" avisa (com som) quando alguém começa ou para;
/// - "Conexão instável" aparece na barra de baixo quando as imagens chegam atrasadas.
public partial class MainWindow
{
    private enum WatchState { None, Connecting, Watching }

    /// Logo depois de entrar na sala chega a lista de quem já transmite: sem aviso nesse tempo.
    private static readonly TimeSpan StreamNoticeGrace = TimeSpan.FromSeconds(3);

    /// O aviso "Conexão instável" fica na barra por este tempo depois do último problema
    /// (para não ficar piscando a cada resumo).
    private static readonly TimeSpan UnstableNoticeHold = TimeSpan.FromSeconds(6);

    /// Largura máxima do nome na lista (o ícone de tela fica logo depois dele).
    private const double UserNameMaxWidth = 150;

    private const string ScreenGlyph = "\uE7F4";

    private static readonly IBrush WatchedBorder = new SolidColorBrush(Color.Parse("#3DDC84"));
    private static readonly IBrush WatchedFill = new SolidColorBrush(Color.Parse("#1F3DDC84"));
    private static readonly IBrush ScreenIconIdle = new SolidColorBrush(Color.Parse("#8A929C"));
    private static readonly IBrush ScreenIconHover = new SolidColorBrush(Color.Parse("#2E343C"));

    /// Quem está transmitindo a TELA na sala (id; 0 = o Host). Eu mesmo não entro aqui
    /// quando sou um Client: o Host não manda esse aviso de volta para quem transmite.
    private readonly HashSet<ulong> _streamers = new();

    /// Quem está com a CÂMERA ligada na sala (a imagem dela aparece no quadrado da pessoa).
    private readonly HashSet<ulong> _cameraUsers = new();

    /// Nomes da lista do Host (para os avisos e a barra de baixo).
    private readonly Dictionary<ulong, string> _hostUserNames = new();

    /// Quem estou assistindo (ou tentando assistir). Null = ninguém.
    private ulong? _watchUser;

    /// O que estou assistindo dessa pessoa: false = a tela, true = a câmera (grande).
    private bool _watchCamera;

    /// Número da tentativa atual (vem no watch_connecting). 0 = ainda não chegou:
    /// avisos com outro número são de uma tentativa antiga e são ignorados.
    private ulong _watchId;

    private WatchState _watchState = WatchState.None;

    /// Até quando mostrar "Conexão instável" na barra de baixo.
    private DateTime _watchUnstableUntil = DateTime.MinValue;

    /// Quando entrei na sala (Host: quando abri; Client: quando conectei).
    private DateTime _roomJoinedAt;

    /// Volta tudo de "assistir" ao começo (ao abrir/fechar a sala ou conectar/desconectar).
    private void ResetWatch()
    {
        _streamers.Clear();
        _cameraUsers.Clear();
        _cameraTileViews.Clear();
        _hostUserNames.Clear();
        _ownPreviewBig = false;
        ResetOwnCamera();
        ClearWatchState();
        UpdateWatchControls();
    }

    private void ClearWatchState()
    {
        _watchUser = null;
        _watchCamera = false;
        _watchId = 0;
        _watchState = WatchState.None;
        _watchUnstableUntil = DateTime.MinValue;
        _videoFps = _videoMbps = 0;
        _videoSize = null;
        _syncMs = null;
    }

    /// Clique no ícone de tela (ou no quadrado da câmera, com `camera` = true): assiste essa
    /// transmissão grande (ou para, se já era ela).
    private void ToggleWatch(ulong userId, bool camera = false)
    {
        if (_watchUser == userId && _watchCamera == camera)
        {
            NativeMethods.WatchStop();
            EndWatch(null);
            return;
        }

        var (ok, error) = NativeMethods.WatchKind(userId, camera);
        if (!ok)
        {
            _infoText.Text = Loc.F("Watch_Error", error ?? "?");
            return;
        }

        ClearWatchState();
        _ownPreviewBig = false; // a transmissão assistida toma o lugar da minha prévia grande
        _watchUser = userId;
        _watchCamera = camera;
        _watchState = WatchState.Connecting;

        // Se estava com a imagem de outra transmissão, ela some; a nova aparece quando chegar
        HidePreview(Loc.F("Watch_Connecting", UserName(userId)));
        RefreshSfxUserLists();
        RefreshWatchInfo();
        UpdateWatchControls();
    }

    /// Parou de assistir (escolha minha, fim da transmissão ou erro). `message` = o que mostrar.
    /// A tela da sala volta (com o quadrado da minha transmissão, se eu estiver transmitindo).
    private void EndWatch(string? message)
    {
        ClearWatchState();

        HidePreview(message ?? IdleStageText());

        // A grade da sala costuma cobrir a mensagem: o motivo (erro, recusa) também vira aviso
        if (message is not null) Notify(message, StreamGlyph);

        RefreshSfxUserLists();
        RefreshWatchInfo();
        UpdateWatchControls();
    }

    /// O que aparece na área grande quando não há imagem.
    private string IdleStageText() =>
        _mode == Mode.Host ? Loc.T("Host_RoomOpenHint") : Loc.T("Watch_PickHint");

    /// Volume, botão 🎉 e tela cheia dependem de estar assistindo.
    private void UpdateWatchControls() => UpdateVolumeControls();

    /// Nome de alguém da sala (para os avisos e a barra de baixo).
    private string UserName(ulong id)
    {
        if (_mode == Mode.Client)
        {
            if (id == HostUserId) return _hostName;
            return _clientUsers.TryGetValue(id, out var name) ? name : Loc.T("Common_Someone");
        }

        return _hostUserNames.TryGetValue(id, out var hostName) ? hostName : Loc.T("Common_Someone");
    }

    /// Pede a imagem mais recente da transmissão assistida e desenha na área do vídeo.
    private void UpdateWatchFrame()
    {
        if (!NativeMethods.WatchFrame(_previewBuffer, out var width, out var height)) return;
        PaintFrame(width, height);
    }

    // ───────────────────────── Eventos ─────────────────────────

    /// Eventos de transmissões e de assistir (iguais no Host e no Client).
    /// true = o evento era deste assunto (já foi tratado).
    private bool HandleWatchEvent(string type, JsonElement e)
    {
        if (HandleOwnCameraEvent(type, e)) return true;

        switch (type)
        {
            // Câmeras dos outros: o motor já pede a versão de 720p sozinho; o quadrado mostra
            // a câmera assim que a primeira imagem chegar
            case "user_streaming" when IsCameraEvent(e):
            {
                var id = e.GetProperty("id").GetUInt64();
                if (id != MySfxUserId) _cameraUsers.Add(id);
                RefreshSfxUserLists();
                return true;
            }

            case "user_stream_stopped" when IsCameraEvent(e):
            {
                var id = e.GetProperty("id").GetUInt64();
                _cameraUsers.Remove(id);
                _cameraTileViews.Remove(id);
                RefreshSfxUserLists();
                return true;
            }

            case "user_streaming":
            {
                var id = e.GetProperty("id").GetUInt64();
                var isNew = _streamers.Add(id);
                if (isNew && id != MySfxUserId && DateTime.UtcNow - _roomJoinedAt > StreamNoticeGrace)
                    Notify(Loc.F("Notify_HostStarted", UserName(id)), StreamGlyph, Sound.StreamStarted);
                RefreshSfxUserLists();
                return true;
            }

            case "user_stream_stopped":
            {
                // Se eu assistia essa pessoa, o watch_ended chega logo em seguida
                var id = e.GetProperty("id").GetUInt64();
                if (_streamers.Remove(id) && id != MySfxUserId)
                    Notify(Loc.F("Notify_StreamEnded", UserName(id)), StreamGlyph, Sound.StreamStarted);
                RefreshSfxUserLists();
                return true;
            }

            case "watch_connecting":
            {
                // A conexão começou: guarda o número dela (só se for de quem eu pedi)
                if (_watchUser == e.GetProperty("user_id").GetUInt64())
                    _watchId = e.GetProperty("watch_id").GetUInt64();
                return true;
            }

            case "watch_started":
                if (!IsCurrentWatch(e)) return true;
                _watchState = WatchState.Watching;
                RefreshWatchInfo();
                UpdateWatchControls();
                return true;

            case "watch_failed":
                if (!IsCurrentWatch(e)) return true;
                EndWatch(Loc.F(
                    "Watch_Failed",
                    UserName(e.GetProperty("user_id").GetUInt64()),
                    e.GetProperty("error").GetString()));
                return true;

            case "watch_ended":
            {
                if (!IsCurrentWatch(e)) return true;

                var name = UserName(e.GetProperty("user_id").GetUInt64());
                var stopped = e.TryGetProperty("stream_stopped", out var s) && s.ValueKind == JsonValueKind.True;
                var local = e.TryGetProperty("local", out var l) && l.ValueKind == JsonValueKind.True;

                // Fim normal: o aviso "Fulano encerrou a transmissão" já saiu no user_stream_stopped
                EndWatch(stopped || local ? null
                    : Loc.F("Watch_Lost", name, e.GetProperty("reason").GetString()));
                return true;
            }

            case "watch_denied":
            {
                var user = e.GetProperty("user_id").GetUInt64();
                if (_watchUser != user) return true;
                EndWatch(DeniedText(e.GetProperty("code").GetString(), user));
                return true;
            }

            case "video_started":
                if (_watchState == WatchState.None) return true;
                _videoSize = $"{e.GetProperty("width").GetInt32()}x{e.GetProperty("height").GetInt32()}";
                RefreshWatchInfo();
                return true;

            case "video_stats":
                if (_watchState == WatchState.None) return true;
                _videoFps = e.GetProperty("fps").GetDouble();
                _videoMbps = e.GetProperty("mbps").GetDouble();
                RefreshWatchInfo();
                return true;

            case "sync_stats":
                if (_watchState == WatchState.None) return true;
                _syncMs = e.TryGetProperty("offset_ms", out var off) && off.ValueKind == JsonValueKind.Number
                    ? off.GetDouble()
                    : null;
                // As imagens chegaram atrasadas ou acumuladas: mostra o aviso por um tempo
                if (e.TryGetProperty("unstable", out var unstable) && unstable.ValueKind == JsonValueKind.True)
                    _watchUnstableUntil = DateTime.UtcNow + UnstableNoticeHold;
                RefreshWatchInfo();
                return true;

            case "video_error":
                _infoText.Text = e.GetProperty("error").GetString() ?? Loc.T("Client_VideoError");
                return true;

            case "audio_error":
                // O vídeo continua; só avisa (uma vez por problema) que o som não está saindo
                Notify(e.GetProperty("error").GetString() ?? Loc.T("Client_AudioError"), AudioErrorGlyph);
                return true;
        }

        return false;
    }

    /// O aviso é da câmera de alguém? (Sem "kind" = versão antiga = tela.)
    private static bool IsCameraEvent(JsonElement e) =>
        e.TryGetProperty("kind", out var kind) && kind.ValueKind == JsonValueKind.String && kind.GetString() == "camera";

    /// O aviso é da tentativa atual de assistir? (Avisos de tentativas antigas são ignorados.)
    private bool IsCurrentWatch(JsonElement e) =>
        _watchUser is { } user
        && e.GetProperty("user_id").GetUInt64() == user
        && e.GetProperty("watch_id").GetUInt64() == _watchId;

    private string DeniedText(string? code, ulong user) => code switch
    {
        "self" => Loc.T("Watch_DeniedSelf"),
        "busy_streaming" => Loc.T("Watch_DeniedBusy"),
        _ => Loc.F("Watch_DeniedNotStreaming", UserName(user)),
    };

    // ───────────────────────── Barra de baixo ─────────────────────────

    /// Atualiza a linha de informações embaixo do vídeo.
    private void RefreshWatchInfo()
    {
        if (_mode == Mode.Client)
        {
            RefreshClientInfo();
            return;
        }

        // No Host a linha junta a transmissão dele e a que ele assiste
        if (_mode == Mode.Host) RefreshHostInfo();
    }

    /// "Assistindo Fulano — 1920x1080 @ 30 FPS, 4.2 Mbps — A/V +3 ms — Conexão instável".
    /// Null = não está assistindo.
    private string? WatchInfoText()
    {
        if (_watchUser is not { } id || _watchState == WatchState.None) return null;

        var parts = new List<string> { Loc.F("Watch_Watching", UserName(id)) };
        if (_videoSize is not null) parts.Add($"{_videoSize} @ {_videoFps:F0} FPS, {_videoMbps:F1} Mbps");
        if (_videoSize is not null && _syncMs is { } sync) parts.Add($"A/V {sync:+0;-0;0} ms");
        if (DateTime.UtcNow < _watchUnstableUntil) parts.Add(Loc.T("Stream_Unstable"));
        return string.Join(" — ", parts);
    }

    // ───────────────────────── Lista de usuários ─────────────────────────

    /// A pessoa está com a câmera ligada? (No meu nome vale o meu estado.)
    private bool HasCameraOn(ulong userId) =>
        userId == MySfxUserId ? _cameraOn : _cameraUsers.Contains(userId);

    /// Ícone de câmera de quem está com ela ligada: cinza; verde quando eu assisto ela grande.
    /// Clicar abre a câmera grande (ou para). No meu nome, só mostra.
    private Control CameraIcon(ulong userId)
    {
        var mine = userId == MySfxUserId;
        var watched = IsWatchingUser(userId) && _watchCamera;

        var icon = new Border
        {
            Width = 26,
            Height = 22,
            CornerRadius = new CornerRadius(5),
            Background = watched ? WatchedFill : Brushes.Transparent,
            VerticalAlignment = VerticalAlignment.Center,
            Child = new TextBlock
            {
                Text = CameraGlyph,
                FontFamily = IconFont,
                FontSize = 14,
                Foreground = watched ? WatchedBorder : ScreenIconIdle,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };

        if (mine)
        {
            ToolTip.SetTip(icon, Loc.T("Camera_OwnTooltip"));
            return icon;
        }

        icon.Cursor = new Cursor(StandardCursorType.Hand);
        ToolTip.SetTip(icon, Loc.T(watched ? "Watch_StopTooltip" : "Room_WatchCameraTooltip"));
        icon.PointerEntered += (_, _) =>
        {
            if (!watched) icon.Background = ScreenIconHover;
        };
        icon.PointerExited += (_, _) =>
        {
            if (!watched) icon.Background = Brushes.Transparent;
        };
        icon.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(icon).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            ToggleWatch(userId, camera: true);
        };

        return icon;
    }

    /// A pessoa está transmitindo? (No meu nome vale o meu estado, não o aviso da sala.)
    private bool IsStreamingUser(ulong userId) =>
        userId == MySfxUserId ? _streaming : _streamers.Contains(userId);

    /// Uma linha da lista da sala: avatar, nome, ícone de tela (se a pessoa transmite) e,
    /// no fim, um botão opcional (Expulsar). Borda azul quando tocou um efeito sonoro;
    /// borda verde quando é quem eu assisto.
    private Control BuildUserRow(Bitmap? avatar, string name, ulong userId, Control? trailing = null)
    {
        var row = new DockPanel { LastChildFill = true };

        if (trailing is not null)
        {
            DockPanel.SetDock(trailing, Dock.Right);
            trailing.VerticalAlignment = VerticalAlignment.Center;
            row.Children.Add(trailing);
        }

        var circle = new Ellipse
        {
            Width = ListAvatarSize,
            Height = ListAvatarSize,
            Fill = AvatarBrush(avatar),
            Margin = new Thickness(0, 0, 10, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Cursor = new Cursor(StandardCursorType.Hand),
        };
        ToolTip.SetTip(circle, Loc.T("Room_OpenTooltip"));

        // Clicar no avatar (transmitindo ou não) volta para a tela da sala
        circle.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(circle).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            ShowRoomView();
        };
        DockPanel.SetDock(circle, Dock.Left);
        row.Children.Add(circle);

        // Nome + ícone de câmera + ícone de tela logo depois dele
        var hasCamera = HasCameraOn(userId);
        var streaming = IsStreamingUser(userId);
        var line = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        line.Children.Add(new TextBlock
        {
            Text = name,
            // Com os dois ícones, o nome encolhe um pouco para caber
            MaxWidth = hasCamera && streaming ? UserNameMaxWidth - 30 : UserNameMaxWidth,
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        if (hasCamera) line.Children.Add(CameraIcon(userId));
        if (streaming) line.Children.Add(ScreenIcon(userId));
        row.Children.Add(line);

        var wrapped = WithSfxHighlight(row, userId);

        // Quem eu assisto: borda verde (a azul do efeito sonoro tem prioridade enquanto o som toca)
        if (IsWatchingUser(userId)
            && wrapped is Border border
            && ReferenceEquals(border.BorderBrush, Brushes.Transparent))
        {
            border.BorderBrush = WatchedBorder;
            border.Background = WatchedFill;
        }

        return wrapped;
    }

    private bool IsWatchingUser(ulong userId) =>
        _watchState != WatchState.None && _watchUser == userId;

    /// Ícone de tela de quem transmite: cinza; verde quando eu assisto.
    /// Clicar assiste (ou para). No meu nome, só mostra.
    private Control ScreenIcon(ulong userId)
    {
        var mine = userId == MySfxUserId;
        var watched = IsWatchingUser(userId) && !_watchCamera;

        var icon = new Border
        {
            Width = 26,
            Height = 22,
            CornerRadius = new CornerRadius(5),
            Background = watched ? WatchedFill : Brushes.Transparent,
            VerticalAlignment = VerticalAlignment.Center,
            Child = new TextBlock
            {
                Text = ScreenGlyph,
                FontFamily = IconFont,
                FontSize = 14,
                Foreground = watched ? WatchedBorder : ScreenIconIdle,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };

        icon.Cursor = new Cursor(StandardCursorType.Hand);

        if (mine)
        {
            // No meu nome: abre a prévia grande da minha transmissão (ou volta para a grade)
            ToolTip.SetTip(icon, Loc.T("Watch_OwnTooltip"));
            icon.PointerEntered += (_, _) => icon.Background = ScreenIconHover;
            icon.PointerExited += (_, _) => icon.Background = Brushes.Transparent;
            icon.PointerPressed += (_, e) =>
            {
                if (!e.GetCurrentPoint(icon).Properties.IsLeftButtonPressed) return;
                e.Handled = true;
                if (_ownPreviewBig) CloseOwnPreviewBig();
                else ShowOwnPreviewBig();
            };
            return icon;
        }

        ToolTip.SetTip(icon, Loc.T(watched ? "Watch_StopTooltip" : "Watch_Tooltip"));

        icon.PointerEntered += (_, _) =>
        {
            if (!watched) icon.Background = ScreenIconHover;
        };
        icon.PointerExited += (_, _) =>
        {
            if (!watched) icon.Background = Brushes.Transparent;
        };
        icon.PointerPressed += (_, e) =>
        {
            if (!e.GetCurrentPoint(icon).Properties.IsLeftButtonPressed) return;
            e.Handled = true;
            ToggleWatch(userId);
        };

        return icon;
    }
}
