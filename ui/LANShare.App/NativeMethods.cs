using System.Runtime.InteropServices;

namespace LANShare.App;

/// Funções exportadas pela biblioteca do Rust (crates/lanshare-ffi → lanshare_ffi.dll).
internal static class NativeMethods
{
    private const string Library = "lanshare_ffi";

    [DllImport(Library, EntryPoint = "lanshare_version", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr lanshare_version();

    /// Versão do motor Rust. O texto vem de memória estática do Rust, então não se libera.
    public static string GetVersion() =>
        Marshal.PtrToStringUTF8(lanshare_version()) ?? "?";
}