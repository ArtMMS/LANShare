using System.Runtime.InteropServices;

namespace LANShare.App;

/// Pedido de energia do Windows (o mesmo que os navegadores usam ao tocar vídeo):
/// enquanto ativo, a tela não apaga, a proteção de tela não liga e o PC não suspende sozinho.
/// Suspender pelo menu ou fechando a tampa continua funcionando.
/// Confira com `powercfg /requests` (prompt de comando como administrador).
internal sealed class PowerRequest : IDisposable
{
    private const uint POWER_REQUEST_CONTEXT_VERSION = 0;
    private const uint POWER_REQUEST_CONTEXT_SIMPLE_STRING = 0x1;

    private enum PowerRequestType
    {
        DisplayRequired = 0,
        SystemRequired = 1,
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct ReasonContext
    {
        public uint Version;
        public uint Flags;
        public IntPtr SimpleReasonString;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr PowerCreateRequest(ref ReasonContext context);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PowerSetRequest(IntPtr handle, PowerRequestType type);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PowerClearRequest(IntPtr handle, PowerRequestType type);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    private static readonly IntPtr InvalidHandle = new(-1);

    private IntPtr _handle;
    private IntPtr _reason;
    private bool _active;

    private PowerRequest(IntPtr handle, IntPtr reason)
    {
        _handle = handle;
        _reason = reason;
    }

    /// Cria o pedido (ainda desligado). Null se o Windows recusar (aí o app só segue sem ele).
    /// `reason` é o texto que aparece no `powercfg /requests`.
    public static PowerRequest? TryCreate(string reason)
    {
        var text = Marshal.StringToHGlobalUni(reason);
        var context = new ReasonContext
        {
            Version = POWER_REQUEST_CONTEXT_VERSION,
            Flags = POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            SimpleReasonString = text,
        };

        try
        {
            var handle = PowerCreateRequest(ref context);
            if (handle == IntPtr.Zero || handle == InvalidHandle)
            {
                Marshal.FreeHGlobal(text);
                return null;
            }

            // O texto fica guardado até o fim (é liberado no Dispose)
            return new PowerRequest(handle, text);
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException)
        {
            Marshal.FreeHGlobal(text);
            return null;
        }
    }

    /// Liga (true) ou desliga (false). Só chama o Windows quando muda.
    public void Set(bool keepAwake)
    {
        if (keepAwake == _active || _handle == IntPtr.Zero) return;

        if (keepAwake)
        {
            PowerSetRequest(_handle, PowerRequestType.DisplayRequired);
            PowerSetRequest(_handle, PowerRequestType.SystemRequired);
        }
        else
        {
            PowerClearRequest(_handle, PowerRequestType.DisplayRequired);
            PowerClearRequest(_handle, PowerRequestType.SystemRequired);
        }

        _active = keepAwake;
    }

    public void Dispose()
    {
        Set(false);

        if (_handle != IntPtr.Zero)
        {
            CloseHandle(_handle);
            _handle = IntPtr.Zero;
        }

        if (_reason != IntPtr.Zero)
        {
            Marshal.FreeHGlobal(_reason);
            _reason = IntPtr.Zero;
        }
    }
}