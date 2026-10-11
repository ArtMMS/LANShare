using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace LANShare.App;

public partial class MainWindow : Window
{
    public MainWindow()
    {
        InitializeComponent();
        ShowCoreStatus();
    }

    private void InitializeComponent() => AvaloniaXamlLoader.Load(this);

    /// Chama o motor Rust e mostra o resultado (prova de que a ponte C# ↔ Rust funciona).
    private void ShowCoreStatus()
    {
        var versionText = this.FindControl<TextBlock>("VersionText")!;
        var statusText = this.FindControl<TextBlock>("StatusText")!;

        try
        {
            var version = NativeMethods.GetVersion();
            versionText.Text = $"Motor Rust v{version}";
            statusText.Text = "Ponte C# ↔ Rust funcionando.";
        }
        catch (DllNotFoundException)
        {
            versionText.Text = "Motor Rust indisponível";
            statusText.Text = "Não achei a lanshare_ffi.dll. Rode: cargo build --release -p lanshare-ffi";
        }
        catch (EntryPointNotFoundException)
        {
            versionText.Text = "Motor Rust incompatível";
            statusText.Text = "A lanshare_ffi.dll não tem a função lanshare_version.";
        }
    }
}