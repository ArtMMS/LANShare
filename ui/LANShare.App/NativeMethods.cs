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
    private static extern IntPtr lanshare_host_start_stream(uint height, uint fps);

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
    private static extern int lanshare_client_disconnect();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_client_poll_event();

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern int lanshare_client_frame(byte[] buffer, UIntPtr capacity, out uint width, out uint height);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)]
    private static extern void lanshare_client_stop();

    /// Versão do motor Rust. O texto vem de memória estática do Rust, então não se libera.
    public static string GetVersion() =>
        Marshal.PtrToStringUTF8(lanshare_version()) ?? "?";

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

    public static (bool Ok, string? Error) HostStartStream(uint height, uint fps) =>
        ParseReply(lanshare_host_start_stream(height, fps));

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
}