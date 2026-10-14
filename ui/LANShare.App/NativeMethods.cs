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
    private static extern void lanshare_host_stop();

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

    public static (bool Ok, string? Error) HostStart(string? password, int? maxClients, string? roomName)
    {
        var json = JsonSerializer.Serialize(new Dictionary<string, object?>
        {
            ["password"] = string.IsNullOrEmpty(password) ? null : password,
            ["max_clients"] = maxClients,
            ["room_name"] = string.IsNullOrWhiteSpace(roomName) ? null : roomName,
        });
        var ptr = Marshal.StringToCoTaskMemUTF8(json);
        try { return ParseReply(lanshare_host_start(ptr)); }
        finally { Marshal.FreeCoTaskMem(ptr); }
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

    public static void HostStop() => lanshare_host_stop();
}