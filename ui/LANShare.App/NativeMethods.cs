using System.Runtime.InteropServices;
using System.Text.Json;

namespace LANShare.App;

/// Funções exportadas pela biblioteca do Rust (crates/lanshare-ffi → lanshare_ffi.dll).
internal static class NativeMethods
{
    private const string Library = "lanshare_ffi";

    [DllImport(Library, EntryPoint = "lanshare_version", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_version();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void lanshare_free_string(IntPtr text);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_set_language(IntPtr code);

    // ── Configurações ──

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_settings_get_username();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_settings_set_username(IntPtr name);

    // ── Host ──

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_start(IntPtr configJson);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_poll_event();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_users();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_kick(ulong id);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_set_avatar(IntPtr pngBase64);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_monitors();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_host_monitor_thumbnail(
        uint index, byte[] buffer, UIntPtr capacity,
        out uint width, out uint height, out uint sourceWidth, out uint sourceHeight);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_windows();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_host_window_thumbnail(
        ulong id, byte[] buffer, UIntPtr capacity,
        out uint width, out uint height, out uint sourceWidth, out uint sourceHeight);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_host_window_icon(ulong id, byte[] buffer, UIntPtr capacity);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_start_stream(uint height, uint fps);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_start_stream_on(uint height, uint fps, int monitor);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_start_stream_window(uint height, uint fps, ulong id);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_switch_monitor(int monitor);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_switch_window(ulong id);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_set_stream_audio(int enabled);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_host_stop_stream();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_host_preview(byte[] buffer, UIntPtr capacity, out uint width, out uint height);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void lanshare_host_stop();

    // ── Client ──

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_start();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_scan();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_connect(IntPtr address);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_submit_password(IntPtr password);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_set_avatar(IntPtr pngBase64);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_set_audio_output(IntPtr deviceId);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_set_volume(uint percent, int muted);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_client_disconnect();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_poll_event();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_client_frame(byte[] buffer, UIntPtr capacity, out uint width, out uint height);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void lanshare_client_stop();

    // ── Áudio ──

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_audio_output_devices();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_audio_apps();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_set_audio_exclusions(IntPtr exesJson);

    // ── Efeitos sonoros ──

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_host_play_sfx(IntPtr name, float[] pcm, UIntPtr len);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern long lanshare_host_take_sfx(ulong sfxId, float[] buffer, UIntPtr capacity);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_play_sfx(IntPtr name, float[] pcm, UIntPtr len);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern long lanshare_client_take_sfx(ulong sfxId, float[] buffer, UIntPtr capacity);

    /// Versão do motor Rust. O texto vem de memória estática do Rust, então não se libera.
    public static string GetVersion() =>
        Marshal.PtrToStringUTF8(lanshare_version()) ?? "?";

    /// Idioma das mensagens do motor ("pt", "en" ou "es"). False se o Rust não aceitou o código.
    public static bool SetLanguage(string code)
    {
        var ptr = Marshal.StringToCoTaskMemUTF8(code);
        try { return lanshare_set_language(ptr) != 0; }
        finally { Marshal.FreeCoTaskMem(ptr); }
    }

    /// Lê um texto alocado pelo Rust e o libera. Null se o ponteiro for nulo.
    private static string? TakeString(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero) return null;
        try { return Marshal.PtrToStringUTF8(ptr); }
        finally { lanshare_free_string(ptr); }
    }

    /// Interpreta {"ok":true} / {"ok":false,"error":".."}.
    private static (bool Ok, string? Error) ParseReply(IntPtr ptr)
    {
        var json = TakeString(ptr) ?? "{}";
        using var doc = JsonDocument.Parse(json);
        var ok = doc.RootElement.TryGetProperty("ok", out var o) && o.GetBoolean();
        var error = doc.RootElement.TryGetProperty("error", out var e) ? e.GetString() : null;
        return (ok, error);
    }

    /// Passa um texto (UTF-8) ao Rust, chama a função e interpreta a resposta.
    private static (bool Ok, string? Error) CallWithString(string text, Func<IntPtr, IntPtr> call)
    {
        var ptr = Marshal.StringToCoTaskMemUTF8(text);
        try { return ParseReply(call(ptr)); }
        finally { Marshal.FreeCoTaskMem(ptr); }
    }

    // ───────────── Configurações ─────────────

    /// Nome de usuário salvo, ou null na primeira abertura.
    public static string? GetUsername()
    {
        var json = TakeString(lanshare_settings_get_username());
        if (json is null) return null;

        using var doc = JsonDocument.Parse(json);
        return doc.RootElement.TryGetProperty("username", out var u) && u.ValueKind == JsonValueKind.String
            ? u.GetString()
            : null;
    }

    /// Salva o nome. Em caso de sucesso devolve o nome já limpo pelo Rust.
    public static (bool Ok, string? Username, string? Error) SetUsername(string? name)
    {
        var ptr = Marshal.StringToCoTaskMemUTF8(name ?? "");
        try
        {
            var json = TakeString(lanshare_settings_set_username(ptr)) ?? "{}";
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            var ok = root.TryGetProperty("ok", out var o) && o.GetBoolean();
            var username = root.TryGetProperty("username", out var u) ? u.GetString() : null;
            var error = root.TryGetProperty("error", out var e) ? e.GetString() : null;
            return (ok, username, error);
        }
        finally { Marshal.FreeCoTaskMem(ptr); }
    }

    // ───────────── Host ─────────────

    public static (bool Ok, string? Error) HostStart(string? password, int? maxClients, string? roomName)
    {
        var json = JsonSerializer.Serialize(new Dictionary<string, object?>
        {
            ["password"] = string.IsNullOrEmpty(password) ? null : password,
            ["max_clients"] = maxClients,
            ["room_name"] = string.IsNullOrWhiteSpace(roomName) ? null : roomName,
        });
        return CallWithString(json, lanshare_host_start);
    }

    /// Próximo evento do Host (JSON) ou null se não há nenhum.
    public static JsonDocument? HostPollEvent()
    {
        var json = TakeString(lanshare_host_poll_event());
        return json is null ? null : JsonDocument.Parse(json);
    }

    public static JsonDocument HostUsers() =>
        JsonDocument.Parse(TakeString(lanshare_host_users()) ?? "[]");

    public static (bool Ok, string? Error) HostKick(ulong id) => ParseReply(lanshare_host_kick(id));

    /// Avatar do Host (PNG em base64; null = sem avatar), enviado a todos na sala.
    public static (bool Ok, string? Error) HostSetAvatar(string? pngBase64) =>
        CallWithString(pngBase64 ?? "", lanshare_host_set_avatar);

    /// Tamanho máximo da miniatura de monitor/janela (igual ao THUMB_MAX do monitors.rs).
    public const int ThumbnailMaxWidth = 400;
    public const int ThumbnailMaxHeight = 240;
    public const int ThumbnailBufferSize = ThumbnailMaxWidth * ThumbnailMaxHeight * 4;

    /// Lado do ícone de janela (igual ao ICON_SIZE do app_windows.rs).
    public const int WindowIconSize = 32;
    public const int WindowIconBufferSize = WindowIconSize * WindowIconSize * 4;

    /// Monitores do computador (principal primeiro): index, name, model, width, height, primary.
    public static JsonDocument HostMonitors() =>
        JsonDocument.Parse(TakeString(lanshare_host_monitors()) ?? "[]");

    /// Tira uma foto reduzida do monitor. Demora uma fração de segundo: chame fora da thread da interface.
    public static bool HostMonitorThumbnail(
        int index, byte[] buffer, out int width, out int height, out int sourceWidth, out int sourceHeight)
    {
        var ok = lanshare_host_monitor_thumbnail(
            (uint)index, buffer, (UIntPtr)buffer.Length,
            out var w, out var h, out var sw, out var sh) != 0;
        width = (int)w;
        height = (int)h;
        sourceWidth = (int)sw;
        sourceHeight = (int)sh;
        return ok;
    }

    /// Janelas que dá para transmitir (a mais recente primeiro): id, title, minimized.
    public static JsonDocument HostWindows() =>
        JsonDocument.Parse(TakeString(lanshare_host_windows()) ?? "[]");

    /// Tira uma foto reduzida da janela. Demora uma fração de segundo: chame fora da thread da interface.
    public static bool HostWindowThumbnail(
        ulong id, byte[] buffer, out int width, out int height, out int sourceWidth, out int sourceHeight)
    {
        var ok = lanshare_host_window_thumbnail(
            id, buffer, (UIntPtr)buffer.Length,
            out var w, out var h, out var sw, out var sh) != 0;
        width = (int)w;
        height = (int)h;
        sourceWidth = (int)sw;
        sourceHeight = (int)sh;
        return ok;
    }

    /// Ícone da janela (32x32 BGRA, alfa pré-multiplicado). False se a janela não tem ícone.
    public static bool HostWindowIcon(ulong id, byte[] buffer) =>
        lanshare_host_window_icon(id, buffer, (UIntPtr)buffer.Length) != 0;

    public static (bool Ok, string? Error) HostStartStream(uint height, uint fps) =>
        ParseReply(lanshare_host_start_stream(height, fps));

    /// Transmite um monitor específico (o "index" da lista; negativo = principal).
    public static (bool Ok, string? Error) HostStartStreamOn(uint height, uint fps, int monitor) =>
        ParseReply(lanshare_host_start_stream_on(height, fps, monitor));

    /// Transmite uma janela específica (o "id" da lista de janelas).
    public static (bool Ok, string? Error) HostStartStreamWindow(uint height, uint fps, ulong id) =>
        ParseReply(lanshare_host_start_stream_window(height, fps, id));

    /// Troca a transmissão em andamento para outro monitor, sem parar.
    public static (bool Ok, string? Error) HostSwitchMonitor(int monitor) =>
        ParseReply(lanshare_host_switch_monitor(monitor));

    /// Troca a transmissão em andamento para uma janela, sem parar.
    public static (bool Ok, string? Error) HostSwitchWindow(ulong id) =>
        ParseReply(lanshare_host_switch_window(id));

    /// Liga ou desliga o som da transmissão (botão 🔊 do seletor).
    /// Vale para a próxima transmissão e, se já estiver transmitindo, muda na hora.
    public static (bool Ok, string? Error) HostSetStreamAudio(bool enabled) =>
        ParseReply(lanshare_host_set_stream_audio(enabled ? 1 : 0));

    public static bool HostStopStream() => lanshare_host_stop_stream() != 0;

    /// Tamanho máximo da imagem (2560x1440, 4 bytes por pixel BGRA).
    public const int PreviewMaxWidth = 2560;
    public const int PreviewMaxHeight = 1440;
    public const int PreviewBufferSize = PreviewMaxWidth * PreviewMaxHeight * 4;

    /// Copia a miniatura da transmissão para `buffer`. False se ainda não há imagem.
    public static bool HostPreview(byte[] buffer, out int width, out int height)
    {
        var ok = lanshare_host_preview(buffer, (UIntPtr)buffer.Length, out var w, out var h) != 0;
        width = (int)w;
        height = (int)h;
        return ok;
    }

    public static void HostStop() => lanshare_host_stop();

    // ───────────── Client ─────────────

    public static (bool Ok, string? Error) ClientStart() => ParseReply(lanshare_client_start());

    public static (bool Ok, string? Error) ClientScan() => ParseReply(lanshare_client_scan());

    public static (bool Ok, string? Error) ClientConnect(string address) =>
        CallWithString(address, lanshare_client_connect);

    public static (bool Ok, string? Error) ClientSubmitPassword(string password) =>
        CallWithString(password, lanshare_client_submit_password);

    /// O meu avatar (PNG em base64; null = sem avatar), enviado ao Host ao entrar numa sala.
    public static (bool Ok, string? Error) ClientSetAvatar(string? pngBase64) =>
        CallWithString(pngBase64 ?? "", lanshare_client_set_avatar);

    /// Por onde sai o som das transmissões (o Id de AudioOutputDevices; null = padrão do Windows).
    /// Se já houver som tocando, ele muda de saída na hora.
    public static (bool Ok, string? Error) ClientSetAudioOutput(string? deviceId) =>
        CallWithString(deviceId ?? "", lanshare_client_set_audio_output);

    /// Volume do som das transmissões (0 a 100) e mudo. Muda na hora, mesmo com som tocando.
    public static (bool Ok, string? Error) ClientSetVolume(int percent, bool muted) =>
        ParseReply(lanshare_client_set_volume((uint)Math.Clamp(percent, 0, 100), muted ? 1 : 0));

    public static bool ClientDisconnect() => lanshare_client_disconnect() != 0;

    /// Próximo evento do Client (JSON) ou null se não há nenhum.
    public static JsonDocument? ClientPollEvent()
    {
        var json = TakeString(lanshare_client_poll_event());
        return json is null ? null : JsonDocument.Parse(json);
    }

    /// Copia a imagem recebida para `buffer`. True só se for uma imagem NOVA desde a última chamada.
    public static bool ClientFrame(byte[] buffer, out int width, out int height)
    {
        var ok = lanshare_client_frame(buffer, (UIntPtr)buffer.Length, out var w, out var h) != 0;
        width = (int)w;
        height = (int)h;
        return ok;
    }

    public static void ClientStop() => lanshare_client_stop();

    // ───────────── Efeitos sonoros ─────────────

    /// Maior efeito sonoro, em amostras float intercaladas (igual ao MAX_SAMPLES do sfx.rs: 10,4 s).
    public const int SfxMaxSamples = 520 * 960 * 2;

    /// Buffer reaproveitado para receber os sons (cada busca copia só o tamanho certo).
    private static readonly float[] SfxBuffer = new float[SfxMaxSamples];
    private static readonly object SfxBufferGate = new();

    /// Toca um efeito sonoro do Host para todos os Clients (PCM float, 48 kHz, estéreo).
    /// Comprime o som: chame fora da thread da interface.
    public static (bool Ok, string? Error) HostPlaySfx(string name, float[] pcm) =>
        PlaySfx(name, pcm, lanshare_host_play_sfx);

    /// Toca um efeito sonoro do Client para a sala (vai ao Host, que repassa a todos).
    /// Comprime o som: chame fora da thread da interface.
    public static (bool Ok, string? Error) ClientPlaySfx(string name, float[] pcm) =>
        PlaySfx(name, pcm, lanshare_client_play_sfx);

    /// Busca o som avisado no evento "sfx_played" (só uma vez). Null = não existe mais.
    /// Descomprime o som: chame fora da thread da interface.
    public static float[]? HostTakeSfx(ulong sfxId) => TakeSfx(sfxId, lanshare_host_take_sfx);

    /// Igual ao HostTakeSfx, para o Client.
    public static float[]? ClientTakeSfx(ulong sfxId) => TakeSfx(sfxId, lanshare_client_take_sfx);

    private static (bool Ok, string? Error) PlaySfx(
        string name, float[] pcm, Func<IntPtr, float[], UIntPtr, IntPtr> play)
    {
        var ptr = Marshal.StringToCoTaskMemUTF8(name);
        try { return ParseReply(play(ptr, pcm, (UIntPtr)pcm.Length)); }
        finally { Marshal.FreeCoTaskMem(ptr); }
    }

    private static float[]? TakeSfx(ulong sfxId, Func<ulong, float[], UIntPtr, long> take)
    {
        lock (SfxBufferGate)
        {
            var count = take(sfxId, SfxBuffer, (UIntPtr)SfxBuffer.Length);
            return count > 0 ? SfxBuffer[..(int)count] : null;
        }
    }

    // ───────────── Áudio ─────────────

    /// Saídas de áudio do PC (o padrão do Windows primeiro). Demora alguns milissegundos:
    /// chame fora da thread da interface. Em caso de erro, a lista vem vazia e Error explica.
    public static (List<AudioOutputDevice> Devices, string? Error) AudioOutputDevices()
    {
        var json = TakeString(lanshare_audio_output_devices()) ?? "{}";
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        var devices = new List<AudioOutputDevice>();

        if (!(root.TryGetProperty("ok", out var ok) && ok.GetBoolean()))
        {
            var error = root.TryGetProperty("error", out var e) ? e.GetString() : null;
            return (devices, error ?? "?");
        }

        if (root.TryGetProperty("devices", out var list) && list.ValueKind == JsonValueKind.Array)
        {
            foreach (var d in list.EnumerateArray())
            {
                var id = d.GetProperty("id").GetString();
                if (string.IsNullOrEmpty(id)) continue;
                devices.Add(new AudioOutputDevice(
                    id,
                    d.GetProperty("name").GetString() ?? id,
                    d.TryGetProperty("default", out var def) && def.ValueKind == JsonValueKind.True));
            }
        }

        return (devices, null);
    }

    /// Apps com som aberto no PC (os que tocam agora primeiro). Demora alguns milissegundos:
    /// chame fora da thread da interface. Em caso de erro, a lista vem vazia e Error explica.
    public static (List<AudioApp> Apps, string? Error) AudioApps()
    {
        var json = TakeString(lanshare_audio_apps()) ?? "{}";
        using var doc = JsonDocument.Parse(json);
        var root = doc.RootElement;
        var apps = new List<AudioApp>();

        if (!(root.TryGetProperty("ok", out var ok) && ok.GetBoolean()))
        {
            var error = root.TryGetProperty("error", out var e) ? e.GetString() : null;
            return (apps, error ?? "?");
        }

        if (root.TryGetProperty("apps", out var list) && list.ValueKind == JsonValueKind.Array)
        {
            foreach (var a in list.EnumerateArray())
            {
                var exe = a.GetProperty("exe").GetString();
                if (string.IsNullOrEmpty(exe)) continue;
                apps.Add(new AudioApp(
                    exe,
                    a.GetProperty("name").GetString() ?? exe,
                    a.TryGetProperty("active", out var act) && act.ValueKind == JsonValueKind.True));
            }
        }

        return (apps, null);
    }

    /// Apps que não transmitem som ao transmitir um monitor inteiro (lista de .exe).
    public static (bool Ok, string? Error) SetAudioExclusions(IEnumerable<string> exes) =>
        CallWithString(JsonSerializer.Serialize(exes.ToList()), lanshare_set_audio_exclusions);
}

/// Um app com som aberto no PC. `Exe` em minúsculas (ex.: "discord.exe"); `Active` = tocando agora.
internal sealed record AudioApp(string Exe, string Name, bool Active);

/// Uma saída de áudio do PC (alto-falantes, fones...). `Id` é o que fica salvo nas preferências.
internal sealed record AudioOutputDevice(string Id, string Name, bool IsDefault);