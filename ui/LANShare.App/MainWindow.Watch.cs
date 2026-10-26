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
/// - quem transmite ganha o ícone de tela ao lado do nome; clicar assiste (clicar de novo para);
/// - quem você assiste fica com a borda verde na lista de usuários;
/// - a transmissão escolhida aparece na área grande, com volume e tela cheia;
/// - "Fulano iniciou a transmissão" avisa (com som) quando alguém começa.
public partial class MainWindow
{
    private enum WatchState { None, Connecting, Watching }

    /// Logo depois de entrar na sala chega a lista de quem já transmite: sem aviso nesse tempo.
    private static readonly TimeSpan StreamNoticeGrace = TimeSpan.FromSeconds(3);

    /// Largura máxima do nome na lista (o ícone de tela fica logo depois dele).
    private const double UserNameMaxWidth = 150;

    private const string ScreenGlyph = "\uE7F4";

    private static readonly IBrush WatchedBorder = new SolidColorBrush(Color.Parse("#3DDC84"));
    private static readonly IBrush WatchedFill = new SolidColorBrush(Color.Parse("#1F3DDC84"));
    private static readonly IBrush ScreenIconHover = new SolidColorBrush(Color.Parse("#2E343C"));

    /// Quem está transmitindo na sala (id; 0 = o Host).
    private readonly HashSet<ulong> _streamers = new();

    /// Nomes da lista do Host (para os avisos e a barra de baixo).
    private readonly Dictionary<ulong, string> _hostUserNames = new();

    /// Quem estou assistindo (ou tentando assistir). Null = ninguém.
    private ulong? _watchUser;

    /// Número da tentativa atual (vem no watch_connecting). 0 = ainda não chegou:
    /// avisos com outro número são de uma tentativa antiga e são ignorados.
    private ulong _watchId;

    private WatchState _watchState = WatchState.None;

    /// Quando entrei na sala (Host: quando abri; Client: quando conectei).
    private DateTime _roomJoinedAt;

    /// Volta tudo de "assistir" ao começo (ao abrir/fechar a sala ou conectar/desconectar).
    private void ResetWatch()
    {
        _streamers.Clear();
        _hostUserNames.Clear();
        ClearWatchState();
        UpdateWatchControls();
    }

    private void ClearWatchState()
    {
        _watchUser = null;
        _watchId = 0;
        _watchState = WatchState.None;
        _videoFps = _videoMbps = 0;
        _videoSize = null;
        _syncMs = null;
    }

    /// Clique no ícone de tela: assiste essa pessoa (ou para, se já era ela).
    private void ToggleWatch(ulong userId)
    {
        if (_watchUser == userId)
        {
            NativeMethods.WatchStop();
            EndWatch(null);
            return;
        }

        var (ok, error) = NativeMethods.Watch(userId);
        if (!ok)
        {
            _infoText.Text = Loc.F("Watch_Error", error ?? "?");
            return;
        }

        ClearWatchState();
        _watchUser = userId;
        _watchState = WatchState.Connecting;

        // Se estava com a prévia de outra coisa, ela some; a imagem nova aparece quando chegar
        HidePreview(Loc.F("Watch_Connecting", UserName(userId)));
        RefreshSfxUserLists();
        RefreshWatchInfo();
        UpdateWatchControls();
    }

    /// Parou de assistir (escolha minha, fim da transmissão ou erro). `message` = o que mostrar.
    private void EndWatch(string? message)
    {
        ClearWatchState();

        // O Host começou a transmitir (e por isso parou de assistir): a prévia dele fica na tela
        if (!(_mode == Mode.Host && _streaming)) HidePreview(message ?? IdleStageText());

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
        switch (type)
        {
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
                // Se eu assistia essa pessoa, o watch_ended chega logo em seguida
                _streamers.Remove(e.GetProperty("id").GetUInt64());
                RefreshSfxUserLists();
                return true;

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

                EndWatch(stopped ? Loc.F("Watch_Ended", name)
                    : local ? null
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

        // No Host, transmitindo, a linha mostra o resumo da transmissão dele
        if (_mode != Mode.Host || _streaming) return;
        _infoText.Text = WatchInfoText() ?? "";
    }

    /// "Assistindo Fulano — 1920x1080 @ 30 FPS, 4.2 Mbps — A/V +3 ms". Null = não está assistindo.
    private string? WatchInfoText()
    {
        if (_watchUser is not { } id || _watchState == WatchState.None) return null;

        var parts = new List<string> { Loc.F("Watch_Watching", UserName(id)) };
        if (_videoSize is not null) parts.Add($"{_videoSize} @ {_videoFps:F0} FPS, {_videoMbps:F1} Mbps");
        if (_videoSize is not null && _syncMs is { } sync) parts.Add($"A/V {sync:+0;-0;0} ms");
        return string.Join(" — ", parts);
    }

    // ───────────────────────── Lista de usuários ─────────────────────────

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
        };
        DockPanel.SetDock(circle, Dock.Left);
        row.Children.Add(circle);

        // Nome + ícone de tela logo depois dele
        var line = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            VerticalAlignment = VerticalAlignment.Center,
        };
        line.Children.Add(new TextBlock
        {
            Text = name,
            MaxWidth = UserNameMaxWidth,
            VerticalAlignment = VerticalAlignment.Center,
            TextTrimming = TextTrimming.CharacterEllipsis,
        });
        if (_streamers.Contains(userId)) line.Children.Add(ScreenIcon(userId));
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

    /// Ícone de tela de quem transmite. Clicar assiste (ou para). No meu nome, só mostra.
    private Control ScreenIcon(ulong userId)
    {
        var mine = userId == MySfxUserId;
        var watched = IsWatchingUser(userId);

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
                Foreground = watched ? WatchedBorder : null,
                HorizontalAlignment = HorizontalAlignment.Center,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };

        if (mine)
        {
            icon.Opacity = 0.6;
            ToolTip.SetTip(icon, Loc.T("Watch_OwnTooltip"));
            return icon;
        }

        icon.Cursor = new Cursor(StandardCursorType.Hand);
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